//! Format serializers — convert serde_json::Value to format strings.
//!
//! Each serializer hands over text that reads back as the data it was given, or refuses.
//! For conversions with educational diagnostics, use the convert module.

use error_core::FormatError;
use serde_json::Value;

/// Serialize Value to pretty-printed JSON.
pub fn to_json(value: &Value) -> Result<String, FormatError> {
    serde_json::to_string_pretty(value)
        .map_err(|e| FormatError::Conversion(format!("JSON serialization failed: {}", e)))
}

/// Serialize Value to YAML, in nornir's one fixed style (`yaml_emit`): every scalar is
/// written so that every mainstream YAML reader types it the same way.
pub fn to_yaml(value: &Value) -> Result<String, FormatError> {
    crate::yaml_emit::emit(value)
}

/// Serialize Value to TOML (raw — no educational diagnostics).
///
/// For conversion with educational errors on failure, use `convert::json_to_toml`.
pub fn to_toml(value: &Value) -> Result<String, FormatError> {
    let toml_value = json_value_to_toml_value(value.clone())
        .map_err(|e| FormatError::Conversion(format!("TOML conversion failed: {}", e)))?;
    toml::to_string_pretty(&toml_value)
        .map_err(|e| FormatError::Conversion(format!("TOML serialization failed: {}", e)))
}

/// Serialize the envelope `{"frontmatter": {...}, "body": "..."}` to a Markdown
/// document — the inverse of `parse::markdown`. Empty frontmatter writes the body
/// alone, unless the body itself opens with a fence; then an empty frontmatter
/// block is written, so the document reads back as the same envelope.
pub fn to_markdown_document(value: &Value) -> Result<String, FormatError> {
    let (frontmatter, body) = envelope_parts(value)?;
    let body_opens_with_fence = body.starts_with("---\n") || body.starts_with("---\r\n");
    if frontmatter.is_empty() && !body_opens_with_fence {
        return Ok(body.to_string());
    }
    if frontmatter.is_empty() {
        return Ok(format!("---\n---\n{body}"));
    }
    let yaml = crate::yaml_emit::emit_mapping(frontmatter)?;
    Ok(format!("---\n{yaml}---\n{body}"))
}

/// The envelope's two parts: an object `frontmatter` and a string `body`.
fn envelope_parts(value: &Value) -> Result<(&serde_json::Map<String, Value>, &str), FormatError> {
    let frontmatter = value.get("frontmatter").and_then(Value::as_object).ok_or_else(|| {
        FormatError::Conversion("a Markdown document needs an object 'frontmatter'".to_string())
    })?;
    let body = value
        .get("body")
        .and_then(Value::as_str)
        .ok_or_else(|| FormatError::Conversion("a Markdown document needs a string 'body'".to_string()))?;
    Ok((frontmatter, body))
}

/// Serialize Value to TOON. The text is handed over only when it reads back as `value`:
/// data TOON would alter — an integer beyond 64 signed bits, a float it writes as
/// something else — is refused, with the path of the first value that changed.
pub fn to_toon(value: &Value) -> Result<String, FormatError> {
    let text = toon_format::encode_default(value)
        .map_err(|e| FormatError::Conversion(format!("TOON serialization failed: {}", e)))?;
    let read = crate::parse::toon(&text);
    crate::write_verify::verified("TOON", value, read, text)
}

/// Serialize Value to XML using @attr/#text convention.
///
/// Expects a top-level object with a single key (the root element name).
/// - `@key` entries become XML attributes
/// - `#text` becomes text content
/// - Arrays become repeated sibling elements
/// - Objects become nested elements
/// - Strings become text-only elements
///
/// The text is handed over only when it reads back as `value`: XML has no types and
/// trims text, so a number, a boolean, a null, an empty or padded string is refused,
/// with the path of the first value that changed.
pub fn to_xml(value: &Value) -> Result<String, FormatError> {
    let obj = value.as_object().ok_or_else(|| {
        FormatError::Conversion("XML requires a root object".to_string())
    })?;

    if obj.len() != 1 {
        return Err(FormatError::Conversion(format!(
            "XML requires exactly one root element, found {} keys: {}",
            obj.len(),
            obj.keys().cloned().collect::<Vec<_>>().join(", ")
        )));
    }

    // Safe: we checked len() == 1 above
    let Some((root_name, root_value)) = obj.iter().next() else {
        return Err(FormatError::Conversion("Empty root object".to_string()));
    };

    if root_name.starts_with('@') {
        return Err(FormatError::Conversion(format!(
            "Root key '{}' looks like an attribute, not an element name",
            root_name
        )));
    }

    check_xml_element(root_name, root_value)?;
    let mut output = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    write_xml_element(&mut output, root_name, root_value, 0)?;
    let read = crate::parse::xml(&output);
    crate::write_verify::verified("XML", value, read, output)
}

/// Every element name, attribute name and character below `value` is one XML 1.0 allows;
/// the first that is not is refused by name.
fn check_xml_element(element_name: &str, value: &Value) -> Result<(), FormatError> {
    check_xml_name(element_name, "element")?;
    match value {
        Value::String(text) => check_xml_characters(text, element_name),
        Value::Array(items) => items.iter().try_for_each(|item| check_xml_element(element_name, item)),
        Value::Object(map) => map.iter().try_for_each(|(key, member)| check_xml_member(element_name, key, member)),
        _ => Ok(()),
    }
}

fn check_xml_member(element_name: &str, key: &str, member: &Value) -> Result<(), FormatError> {
    if let Some(attribute_name) = key.strip_prefix('@') {
        check_xml_name(attribute_name, "attribute")?;
        return member.as_str().map_or(Ok(()), |text| check_xml_characters(text, key));
    }
    if key == "#text" {
        return member.as_str().map_or(Ok(()), |text| check_xml_characters(text, element_name));
    }
    check_xml_element(key, member)
}

fn check_xml_name(name: &str, kind: &str) -> Result<(), FormatError> {
    let mut characters = name.chars();
    let starts = characters.next().is_some_and(is_xml_name_start);
    if starts && characters.all(is_xml_name_character) {
        return Ok(());
    }
    Err(FormatError::Conversion(format!("'{name}' is not a name XML allows for an {kind}")))
}

fn is_xml_name_start(character: char) -> bool {
    matches!(character,
        ':' | '_' | 'A'..='Z' | 'a'..='z'
        | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
        | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}'
        | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
}

fn is_xml_name_character(character: char) -> bool {
    is_xml_name_start(character)
        || matches!(character, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
}

fn check_xml_characters(text: &str, holder: &str) -> Result<(), FormatError> {
    let allowed = |character: char| {
        matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
    };
    match text.chars().find(|character| !allowed(*character)) {
        None => Ok(()),
        Some(character) => Err(FormatError::Conversion(format!(
            "the text of '{holder}' holds the character U+{:04X}, which XML cannot carry",
            character as u32
        ))),
    }
}

/// Classified members of a JSON object for XML serialization.
struct XmlObjectParts<'a> {
    attrs: Vec<(String, String)>,
    text: Option<&'a str>,
    children: Vec<(&'a str, &'a Value)>,
}

/// Classify JSON object keys into attributes (@), text (#text), and child elements.
fn classify_xml_object(map: &serde_json::Map<String, Value>) -> XmlObjectParts<'_> {
    let mut parts = XmlObjectParts {
        attrs: Vec::new(),
        text: None,
        children: Vec::new(),
    };

    for (key, val) in map {
        if let Some(attr_name) = key.strip_prefix('@') {
            let attr_val = match val {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            parts.attrs.push((attr_name.to_string(), attr_val));
        } else if key == "#text" {
            if let Value::String(s) = val {
                parts.text = Some(s.as_str());
            }
        } else {
            parts.children.push((key.as_str(), val));
        }
    }

    parts
}

/// Write the opening tag with attributes. Does not close the tag.
fn write_xml_open_tag(output: &mut String, element_name: &str, indent: &str, attrs: &[(String, String)]) {
    output.push_str(&format!("{}<{}", indent, element_name));
    for (attr_name, attr_val) in attrs {
        output.push_str(&format!(" {}=\"{}\"", attr_name, escape_xml_attr(attr_val)));
    }
}

/// Write child elements (handles arrays as repeated siblings).
fn write_xml_children(
    output: &mut String,
    children: &[(&str, &Value)],
    depth: usize,
) -> Result<(), FormatError> {
    for (child_name, child_value) in children {
        if let Value::Array(items) = child_value {
            for item in items {
                write_xml_element(output, child_name, item, depth)?;
            }
        } else {
            write_xml_element(output, child_name, child_value, depth)?;
        }
    }
    Ok(())
}

/// Write a single XML element — dispatches by JSON value type.
fn write_xml_element(
    output: &mut String,
    element_name: &str,
    value: &Value,
    depth: usize,
) -> Result<(), FormatError> {
    let indent = "  ".repeat(depth);

    match value {
        Value::String(text) => {
            output.push_str(&format!("{}<{}>{}</{}>\n", indent, element_name, escape_xml_text(text), element_name));
        }
        Value::Number(num) => {
            output.push_str(&format!("{}<{}>{}</{}>\n", indent, element_name, num, element_name));
        }
        Value::Bool(val) => {
            output.push_str(&format!("{}<{}>{}</{}>\n", indent, element_name, val, element_name));
        }
        Value::Null => {
            output.push_str(&format!("{}<{}/>\n", indent, element_name));
        }
        Value::Array(items) => {
            for item in items {
                write_xml_element(output, element_name, item, depth)?;
            }
        }
        Value::Object(map) => {
            write_xml_object(output, element_name, map, &indent, depth)?;
        }
    }
    Ok(())
}

/// Write a JSON object as an XML element with attributes, text, and children.
fn write_xml_object(
    output: &mut String,
    element_name: &str,
    map: &serde_json::Map<String, Value>,
    indent: &str,
    depth: usize,
) -> Result<(), FormatError> {
    let parts = classify_xml_object(map);

    write_xml_open_tag(output, element_name, indent, &parts.attrs);

    // Self-closing: no text and no children
    if parts.text.is_none() && parts.children.is_empty() {
        output.push_str("/>\n");
        return Ok(());
    }

    // Text-only (no child elements): inline on same line
    if parts.children.is_empty() {
        if let Some(text) = parts.text {
            output.push_str(&format!(">{}</{}>\n", escape_xml_text(text), element_name));
        }
        return Ok(());
    }

    // Has child elements (and possibly text)
    output.push_str(">\n");
    if let Some(text) = parts.text {
        output.push_str(&format!("{}  {}\n", indent, escape_xml_text(text)));
    }
    write_xml_children(output, &parts.children, depth + 1)?;
    output.push_str(&format!("{}</{}>\n", indent, element_name));
    Ok(())
}

/// Escape special characters in XML text content. A carriage return is written as a
/// character reference: written raw, a reader turns it into a line feed.
fn escape_xml_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\r', "&#13;")
}

/// Escape special characters in XML attribute values. Line breaks and tabs are written
/// as character references: written raw, a reader turns each into a space.
fn escape_xml_attr(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\t', "&#9;")
        .replace('\n', "&#10;")
        .replace('\r', "&#13;")
}

// =============================================================================
// Markdown serialization (one-way, presentation format)
// =============================================================================

/// Serialize Value to human-readable Markdown.
///
/// One-way serializer for viewing structured data as a readable document.
/// Objects become sections with headings, arrays become lists,
/// `@` keys render as metadata, `#text` renders as paragraph text.
pub fn to_markdown(value: &Value) -> Result<String, FormatError> {
    let mut output = String::new();
    match value {
        Value::Object(map) => {
            // If single top-level key, use it as document title
            if map.len() == 1 {
                let Some((key, val)) = map.iter().next() else {
                    return Ok(String::new());
                };
                let title = format_md_heading(key);
                output.push_str(&format!("# {}\n\n", title));
                write_md_attrs(&mut output, val);
                write_md_body(&mut output, val, 2)?;
            } else {
                write_md_object(&mut output, map, 1)?;
            }
        }
        Value::Array(items) => {
            write_md_array(&mut output, items, "items", 1)?;
        }
        _ => {
            output.push_str(&format_md_value(value));
            output.push('\n');
        }
    }
    Ok(output)
}

/// Write `@` attribute keys from an object as a metadata line.
fn write_md_attrs(output: &mut String, value: &Value) {
    let Some(map) = value.as_object() else { return };
    let attrs: Vec<String> = map.iter()
        .filter(|(key, _)| key.starts_with('@'))
        .map(|(key, val)| format!("**{}:** {}", &key[1..], format_md_value(val)))
        .collect();
    if !attrs.is_empty() {
        output.push_str(&attrs.join(" · "));
        output.push_str("\n\n");
    }
}

/// Write the body of an object (non-attribute, non-text keys) as markdown sections.
fn write_md_body(output: &mut String, value: &Value, depth: usize) -> Result<(), FormatError> {
    let Some(map) = value.as_object() else { return Ok(()) };

    // Write #text as a paragraph first
    if let Some(Value::String(text)) = map.get("#text") {
        output.push_str(text.trim());
        output.push_str("\n\n");
    }

    // Write child elements
    for (key, val) in map {
        if key.starts_with('@') || key == "#text" {
            continue;
        }
        write_md_entry(output, key, val, depth)?;
    }
    Ok(())
}

/// Write a single key-value entry as a markdown section or inline value.
fn write_md_entry(
    output: &mut String,
    key: &str,
    value: &Value,
    depth: usize,
) -> Result<(), FormatError> {
    match value {
        Value::String(text) => {
            let heading = heading_prefix(depth);
            let title = format_md_heading(key);
            output.push_str(&format!("{} {}\n\n", heading, title));
            output.push_str(text.trim());
            output.push_str("\n\n");
        }
        Value::Object(map) => {
            let heading = heading_prefix(depth);
            let title = format_md_heading(key);
            output.push_str(&format!("{} {}\n\n", heading, title));
            write_md_attrs(output, value);
            write_md_body(output, value, depth + 1)?;
            // Handle objects with only simple values as a definition list
            let simple_children: Vec<_> = map.iter()
                .filter(|(k, v)| !k.starts_with('@') && k.as_str() != "#text" && is_leaf_value(v))
                .collect();
            if !simple_children.is_empty() && simple_children.len() == map.iter().filter(|(k, _)| !k.starts_with('@') && k.as_str() != "#text").count() {
                // All children are leaves — already handled in write_md_body? No, write_md_body calls write_md_entry.
                // This branch won't re-trigger because the String match above handles leaves.
            }
        }
        Value::Array(items) => {
            write_md_array(output, items, key, depth)?;
        }
        Value::Number(_) | Value::Bool(_) => {
            output.push_str(&format!("**{}:** {}\n\n", format_md_heading(key), format_md_value(value)));
        }
        Value::Null => {
            output.push_str(&format!("**{}:** *(empty)*\n\n", format_md_heading(key)));
        }
    }
    Ok(())
}

/// Write an array as either a bullet list (for simple items) or sub-sections (for objects).
fn write_md_array(
    output: &mut String,
    items: &[Value],
    key: &str,
    depth: usize,
) -> Result<(), FormatError> {
    let heading = heading_prefix(depth);
    let title = format_md_heading(key);
    output.push_str(&format!("{} {}\n\n", heading, title));

    let all_simple = items.iter().all(is_leaf_value);
    if all_simple {
        for item in items {
            output.push_str(&format!("- {}\n", format_md_value(item)));
        }
        output.push('\n');
    } else {
        for (idx, item) in items.iter().enumerate() {
            match item {
                Value::Object(_) => {
                    write_md_attrs(output, item);
                    write_md_body(output, item, depth + 1)?;
                    if idx < items.len() - 1 {
                        output.push_str("---\n\n");
                    }
                }
                _ => {
                    output.push_str(&format!("- {}\n", format_md_value(item)));
                }
            }
        }
        output.push('\n');
    }
    Ok(())
}

/// Write all entries of a top-level object as sections.
fn write_md_object(
    output: &mut String,
    map: &serde_json::Map<String, Value>,
    depth: usize,
) -> Result<(), FormatError> {
    // Write attributes first
    let attrs: Vec<String> = map.iter()
        .filter(|(key, _)| key.starts_with('@'))
        .map(|(key, val)| format!("**{}:** {}", &key[1..], format_md_value(val)))
        .collect();
    if !attrs.is_empty() {
        output.push_str(&attrs.join(" · "));
        output.push_str("\n\n");
    }

    // Write #text
    if let Some(Value::String(text)) = map.get("#text") {
        output.push_str(text.trim());
        output.push_str("\n\n");
    }

    // Write child entries
    for (key, val) in map {
        if key.starts_with('@') || key == "#text" {
            continue;
        }
        write_md_entry(output, key, val, depth)?;
    }
    Ok(())
}

/// Format a key name as a readable heading (underscores → spaces, title case first word).
fn format_md_heading(key: &str) -> String {
    key.replace('_', " ").replace('-', " ")
}

/// Format a leaf value as an inline string.
fn format_md_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "*(empty)*".to_string(),
        _ => value.to_string(),
    }
}

/// Check if a value is a leaf (string, number, bool, null).
fn is_leaf_value(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null)
}

/// Generate a heading prefix: # for 1, ## for 2, capped at ######.
fn heading_prefix(depth: usize) -> String {
    let level = depth.min(6);
    "#".repeat(level)
}

// =============================================================================
// JSON → TOML value conversion
// =============================================================================

pub(crate) fn json_value_to_toml_value(
    value: serde_json::Value,
) -> Result<toml::Value, String> {
    match value {
        Value::Null => Err("TOML does not support null values".to_string()),
        Value::Bool(b) => Ok(toml::Value::Boolean(b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(toml::Value::Integer(i))
            } else if n.is_u64() {
                Err(format!("the integer {n} is beyond TOML's largest integer, 9223372036854775807"))
            } else if let Some(f) = n.as_f64() {
                Ok(toml::Value::Float(f))
            } else {
                Err(format!("Cannot convert number {} to TOML", n))
            }
        }
        Value::String(s) => Ok(toml::Value::String(s)),
        Value::Array(arr) => {
            let toml_arr: Result<Vec<toml::Value>, String> =
                arr.into_iter().map(json_value_to_toml_value).collect();
            Ok(toml::Value::Array(toml_arr?))
        }
        Value::Object(map) => {
            let mut table = toml::map::Map::new();
            for (k, v) in map {
                table.insert(k, json_value_to_toml_value(v)?);
            }
            Ok(toml::Value::Table(table))
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_to_json() {
        let value = json!({"key": "value"});
        let result = to_json(&value).unwrap();
        assert!(result.contains("\"key\""));
        assert!(result.contains("\"value\""));
    }

    #[test]
    fn test_to_yaml() {
        let value = json!({"key": "value"});
        let result = to_yaml(&value).unwrap();
        assert!(result.contains("key:"));
    }

    #[test]
    fn test_to_toml() {
        let value = json!({"key": "value"});
        let result = to_toml(&value).unwrap();
        assert!(result.contains("key = "));
    }

    #[test]
    fn test_to_toml_null_fails() {
        let value = json!({"key": null});
        let err = to_toml(&value).unwrap_err();
        assert!(matches!(err, FormatError::Conversion(_)));
    }

    #[test]
    fn test_to_toml_integer_beyond_its_range_fails() {
        let err = to_toml(&json!({"key": 9223372036854775808_u64})).unwrap_err();
        assert!(err.to_string().contains("beyond TOML's largest integer"), "{err}");
        assert!(to_toml(&json!({"key": 9223372036854775807_i64})).is_ok());
    }

    #[test]
    fn test_to_xml_refuses_names_and_characters_xml_does_not_allow() {
        for value in [
            json!({"with space": "x"}),
            json!({"root": {"occurrence/birth": "x"}}),
            json!({"root": {"1abc": "x"}}),
            json!({"root": {"@bad name": "x"}}),
            json!({"root": {"a": "nul\u{0}"}}),
            json!({"root": {"@k": "bell\u{7}"}}),
        ] {
            assert!(matches!(to_xml(&value).unwrap_err(), FormatError::Conversion(_)), "{value}");
        }
    }

    #[test]
    fn test_to_xml_line_breaks_survive_in_attributes_and_text() {
        let value = json!({"root": {"@k": "a\nb\tc\rd", "#text": "x\ry", "child": "z"}});
        let text = to_xml(&value).unwrap();
        assert!(text.contains("k=\"a&#10;b&#9;c&#13;d\""), "{text}");
        assert_eq!(crate::parse::xml(&text).unwrap(), value);
    }

    #[test]
    fn test_to_toon() {
        let value = json!({"key": "value"});
        let result = to_toon(&value).unwrap();
        assert!(result.contains("key:"));
    }

    #[test]
    fn test_to_toml_preserves_key_order() {
        // Keys deliberately not alphabetical — z before a before m
        let json_str = r#"{"zebra": 1, "alpha": 2, "mango": 3}"#;
        let value: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let toml_str = to_toml(&value).unwrap();
        let z_pos = toml_str.find("zebra").unwrap();
        let a_pos = toml_str.find("alpha").unwrap();
        let m_pos = toml_str.find("mango").unwrap();
        assert!(
            z_pos < a_pos && a_pos < m_pos,
            "key order not preserved: z={z_pos} a={a_pos} m={m_pos}\n{toml_str}"
        );
    }

    #[test]
    fn test_to_toml_preserves_nested_key_order() {
        let json_str = r#"{"outer": {"charlie": 1, "bravo": 2, "alpha": 3}}"#;
        let value: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let toml_str = to_toml(&value).unwrap();
        let c_pos = toml_str.find("charlie").unwrap();
        let b_pos = toml_str.find("bravo").unwrap();
        let a_pos = toml_str.find("alpha").unwrap();
        assert!(
            c_pos < b_pos && b_pos < a_pos,
            "nested key order not preserved: c={c_pos} b={b_pos} a={a_pos}\n{toml_str}"
        );
    }

    #[test]
    fn test_to_toon_with_array() {
        let value = json!({
            "items": [
                {"name": "a", "count": 1},
                {"name": "b", "count": 2}
            ]
        });
        let result = to_toon(&value).unwrap();
        // TOON should encode this more compactly than JSON
        assert!(result.len() < serde_json::to_string_pretty(&value).unwrap().len());
    }
}
