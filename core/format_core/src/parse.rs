//! Format parsers — convert source format strings to serde_json::Value.
//!
//! All parsers produce serde_json::Value as the universal intermediate representation.

use error_core::FormatError;
use serde_json::Value;

/// Parse JSON string to Value.
pub fn json(content: &str) -> Result<Value, FormatError> {
    serde_json::from_str(content).map_err(|e| FormatError::JsonParse(e.to_string()))
}

/// Parse YAML string to Value.
pub fn yaml(content: &str) -> Result<Value, FormatError> {
    serde_yaml::from_str(content).map_err(|e| FormatError::YamlParse(e.to_string()))
}

/// Parse TOML string to Value (via toml::Value intermediate).
pub fn toml(content: &str) -> Result<Value, FormatError> {
    let toml_value: toml::Value =
        toml::from_str(content).map_err(|e| FormatError::TomlParse(e.to_string()))?;
    toml_value_to_json_value(toml_value)
}

/// Parse TOON string to Value.
pub fn toon(content: &str) -> Result<Value, FormatError> {
    toon_format::decode_default(content).map_err(|e| FormatError::ToonParse(e.to_string()))
}

/// Parse XML string to Value using @attr/#text convention.
///
/// Convention:
/// - Attributes become `@attr_name` keys
/// - Text content becomes `#text` (or bare string if no attrs/children)
/// - Repeated same-name siblings become arrays
/// - Root element becomes the top-level key: `{"root": {...}}`
pub fn xml(content: &str) -> Result<Value, FormatError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    // Parse expects a single root element → {"root_name": {...}}
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let attrs = parse_xml_attributes(e)
                    .map_err(|e| FormatError::XmlParse(e))?;
                let children = parse_xml_children(&mut reader, &tag)
                    .map_err(|e| FormatError::XmlParse(e))?;
                let element = build_xml_element(attrs, children);
                let mut root = serde_json::Map::new();
                root.insert(tag, element);
                return Ok(Value::Object(root));
            }
            Ok(Event::Decl(_)) | Ok(Event::Comment(_)) | Ok(Event::PI(_)) => continue,
            Ok(Event::Eof) => {
                return Err(FormatError::XmlParse("Empty XML document".to_string()));
            }
            Ok(event) => {
                return Err(FormatError::XmlParse(format!(
                    "Unexpected event at document root: {:?}", event
                )));
            }
            Err(e) => return Err(FormatError::XmlParse(e.to_string())),
        }
    }
}

/// Parse attributes from an XML start element into a vec of (@key, value) pairs.
fn parse_xml_attributes(
    start: &quick_xml::events::BytesStart,
) -> Result<Vec<(String, String)>, String> {
    let mut attrs = Vec::new();
    for attr in start.attributes() {
        let attr = attr.map_err(|e| format!("Attribute error: {}", e))?;
        let key = format!("@{}", String::from_utf8_lossy(attr.key.as_ref()));
        let val = attr
            .unescape_value()
            .map_err(|e| format!("Attribute decode error: {}", e))?
            .to_string();
        attrs.push((key, val));
    }
    Ok(attrs)
}

/// Parse children of an XML element until its closing tag.
/// Returns a list of (tag_name, Value) pairs for child elements,
/// plus any accumulated text content.
fn parse_xml_children(
    reader: &mut quick_xml::Reader<&[u8]>,
    parent_tag: &str,
) -> Result<(Vec<(String, Value)>, Option<String>), String> {
    use quick_xml::events::Event;

    let mut children: Vec<(String, Value)> = Vec::new();
    let mut text_parts: Vec<String> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let attrs = parse_xml_attributes(e)?;
                let (sub_children, sub_text) = parse_xml_children(reader, &tag)?;
                let element = build_xml_element(attrs, (sub_children, sub_text));
                children.push((tag, element));
            }
            Ok(Event::Empty(ref e)) => {
                // Self-closing element: <foo bar="baz"/>
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let attrs = parse_xml_attributes(e)?;
                let element = build_xml_element(attrs, (vec![], None));
                children.push((tag, element));
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape()
                    .map_err(|e| format!("Text decode error: {}", e))?
                    .to_string();
                if !text.is_empty() {
                    text_parts.push(text);
                }
            }
            Ok(Event::CData(ref e)) => {
                let text = String::from_utf8_lossy(e.as_ref()).to_string();
                if !text.is_empty() {
                    text_parts.push(text);
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == parent_tag {
                    let text = if text_parts.is_empty() {
                        None
                    } else {
                        Some(text_parts.join(""))
                    };
                    return Ok((children, text));
                }
                return Err(format!(
                    "Mismatched closing tag: expected </{}>, got </{}>",
                    parent_tag, tag
                ));
            }
            Ok(Event::Comment(_)) | Ok(Event::PI(_)) | Ok(Event::Decl(_)) | Ok(Event::DocType(_)) => continue,
            Ok(Event::Eof) => {
                return Err(format!("Unexpected EOF inside <{}>", parent_tag));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Build a serde_json::Value from attributes and parsed children.
///
/// Rules:
/// - Text-only, no attrs, no children → bare string
/// - Has attrs or children → object with @attr keys, #text for text, child elements as keys
/// - Repeated same-name children → array
fn build_xml_element(
    attrs: Vec<(String, String)>,
    children: (Vec<(String, Value)>, Option<String>),
) -> Value {
    let (child_elements, text) = children;

    // Simple case: text-only leaf with no attributes and no child elements
    if attrs.is_empty() && child_elements.is_empty() {
        return match text {
            Some(t) => Value::String(t),
            None => Value::Object(serde_json::Map::new()),
        };
    }

    let mut map = serde_json::Map::new();

    // Insert attributes first (preserves visual order: attrs, then children)
    for (key, val) in attrs {
        map.insert(key, Value::String(val));
    }

    // Insert text content
    if let Some(t) = text {
        map.insert("#text".to_string(), Value::String(t));
    }

    // Insert child elements — group repeated names into arrays
    for (name, value) in child_elements {
        if let Some(existing) = map.remove(&name) {
            // Already seen this name — convert to array or push to existing array
            match existing {
                Value::Array(mut arr) => {
                    arr.push(value);
                    map.insert(name, Value::Array(arr));
                }
                _ => {
                    map.insert(name, Value::Array(vec![existing, value]));
                }
            }
        } else {
            map.insert(name, value);
        }
    }

    Value::Object(map)
}

// =============================================================================
// Markdown with YAML frontmatter
// =============================================================================

/// Parse a Markdown document with optional YAML frontmatter to Value.
///
/// Envelope: `{"frontmatter": {...}, "body": "..."}`.
/// - The frontmatter is the YAML block between an opening `---` line at the
///   very start of the document and the next `---` line, parsed with [`yaml`].
/// - No opening fence → `frontmatter` is an empty object, `body` is the whole
///   document. An empty block is also an empty object.
/// - An opening fence with no closing fence is a parse error.
/// - The body is everything after the closing fence line, verbatim.
pub fn markdown(content: &str) -> Result<Value, FormatError> {
    let (frontmatter_text, body) = split_frontmatter(content)?;
    let frontmatter = match frontmatter_text {
        None => Value::Object(serde_json::Map::new()),
        Some(text) => frontmatter_object(yaml(text)?),
    };
    let mut envelope = serde_json::Map::new();
    envelope.insert("frontmatter".to_string(), frontmatter);
    envelope.insert("body".to_string(), Value::String(body.to_string()));
    Ok(Value::Object(envelope))
}

/// Split a document into (frontmatter YAML text, body). `None` frontmatter
/// means no opening fence. The fences are `---` lines: the opening one at
/// byte 0, the closing one the first subsequent line equal to `---`.
fn split_frontmatter(content: &str) -> Result<(Option<&str>, &str), FormatError> {
    let Some(after_open) = content.strip_prefix("---\n") else {
        return Ok((None, content));
    };
    if let Some(body) = after_open.strip_prefix("---\n") {
        return Ok((Some(""), body));
    }
    if let Some(close_at) = after_open.find("\n---\n") {
        return Ok((Some(&after_open[..close_at]), &after_open[close_at + 5..]));
    }
    if let Some(yaml_text) = after_open.strip_suffix("\n---") {
        return Ok((Some(yaml_text), ""));
    }
    Err(FormatError::MarkdownParse(
        "opening frontmatter fence without a closing fence".to_string(),
    ))
}

/// An empty or whitespace-only block parses as YAML null; the envelope
/// promises an object, so null becomes `{}`. Any other shape passes through
/// for the schema to judge.
fn frontmatter_object(parsed: Value) -> Value {
    match parsed {
        Value::Null => Value::Object(serde_json::Map::new()),
        other => other,
    }
}

// =============================================================================
// TOML → JSON value conversion
// =============================================================================

fn toml_value_to_json_value(value: toml::Value) -> Result<Value, FormatError> {
    Ok(match value {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::Number(i.into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(dt) => Value::String(dt.to_string()),
        toml::Value::Array(arr) => {
            let items: Result<Vec<Value>, FormatError> =
                arr.into_iter().map(toml_value_to_json_value).collect();
            Value::Array(items?)
        }
        toml::Value::Table(table) => {
            let mut map = serde_json::Map::new();
            for (k, v) in table {
                map.insert(k, toml_value_to_json_value(v)?);
            }
            Value::Object(map)
        }
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json() {
        let result = json(r#"{"key": "value"}"#).unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn test_parse_yaml() {
        let result = yaml("key: value").unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn test_parse_toml() {
        let result = toml("key = \"value\"").unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn test_parse_toon() {
        // TOON with simple key-value
        let result = toon("key: value").unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn test_parse_json_error() {
        let err = json("not valid json").unwrap_err();
        assert!(matches!(err, FormatError::JsonParse(_)));
    }

    #[test]
    fn test_parse_yaml_error() {
        let err = yaml("invalid: yaml: [unterminated").unwrap_err();
        assert!(matches!(err, FormatError::YamlParse(_)));
    }

    #[test]
    fn test_parse_toml_error() {
        let err = toml("this is not [valid toml").unwrap_err();
        assert!(matches!(err, FormatError::TomlParse(_)));
    }

    #[test]
    fn test_parse_markdown_with_frontmatter() {
        let input = "---\ntitle: Nunavut\ntags: [Arctic]\n---\n\nThe body.\n";
        let result = markdown(input).unwrap();
        assert_eq!(result["frontmatter"]["title"], "Nunavut");
        assert_eq!(result["frontmatter"]["tags"][0], "Arctic");
        assert_eq!(result["body"], "\nThe body.\n");
    }

    #[test]
    fn test_parse_markdown_without_frontmatter() {
        let result = markdown("Just prose.\n").unwrap();
        assert_eq!(result["frontmatter"], serde_json::json!({}));
        assert_eq!(result["body"], "Just prose.\n");
    }

    #[test]
    fn test_parse_markdown_empty_block() {
        let result = markdown("---\n---\nbody").unwrap();
        assert_eq!(result["frontmatter"], serde_json::json!({}));
        assert_eq!(result["body"], "body");
    }

    #[test]
    fn test_parse_markdown_closing_fence_at_eof() {
        let result = markdown("---\ntitle: x\n---").unwrap();
        assert_eq!(result["frontmatter"]["title"], "x");
        assert_eq!(result["body"], "");
    }

    #[test]
    fn test_parse_markdown_unterminated_fence() {
        let err = markdown("---\ntitle: x\nno closing fence\n").unwrap_err();
        assert!(matches!(err, FormatError::MarkdownParse(_)));
    }

    #[test]
    fn test_parse_markdown_date_stays_string() {
        let result = markdown("---\noccurrence/effective: 1999-04-01\n---\n").unwrap();
        assert_eq!(result["frontmatter"]["occurrence/effective"], "1999-04-01");
    }

    #[test]
    fn test_parse_toml_nested() {
        let input = r#"
[metadata]
name = "test"
version = "1.0"

[[items]]
id = 1
label = "first"
"#;
        let result = toml(input).unwrap();
        assert_eq!(result["metadata"]["name"], "test");
        assert_eq!(result["items"][0]["id"], 1);
    }
}
