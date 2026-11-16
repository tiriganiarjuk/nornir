//! The YAML document nornir writes: one fixed block style, laid out as js-yaml lays it out.
//!
//! Mappings and sequences are block style, two spaces a level; a sequence under a key
//! starts at the key's own column; an empty mapping is `{}` and an empty sequence `[]`.
//! Scalars are rendered by [`crate::yaml_scalar`]. A document in this style reads back as
//! the data it was written from, and writes again as the same bytes.

use error_core::FormatError;
use serde_json::{Map, Value};

use crate::yaml_scalar::{render_number, render_string};

/// The longest key a YAML reader accepts in the `key: value` form.
const LONGEST_IMPLICIT_KEY: usize = 1024;

/// Where a node sits: how deep, whether its first line continues the line it is on, and
/// whether it is an item of a sequence.
#[derive(Clone, Copy)]
struct Place {
    level: usize,
    compact: bool,
    in_sequence: bool,
}

/// A whole YAML document for `value`, ending in a line break.
pub fn emit(value: &Value) -> Result<String, FormatError> {
    let mut document = node(value, Place { level: 0, compact: true, in_sequence: false })?;
    document.push('\n');
    Ok(document)
}

/// A whole YAML document for a mapping — the frontmatter of a Markdown document.
pub fn emit_mapping(map: &Map<String, Value>) -> Result<String, FormatError> {
    if map.is_empty() {
        return Ok("{}\n".to_string());
    }
    let mut document = mapping(map, 0, true)?;
    document.push('\n');
    Ok(document)
}

fn node(value: &Value, place: Place) -> Result<String, FormatError> {
    match value {
        Value::Null => Ok("null".to_string()),
        Value::Bool(flag) => Ok(flag.to_string()),
        Value::Number(number) => render_number(number),
        Value::String(text) => Ok(render_string(text, 2 * place.level.max(1), false)),
        Value::Array(items) if items.is_empty() => Ok("[]".to_string()),
        Value::Array(items) => sequence(items, sequence_level(place), place.compact),
        Value::Object(map) if map.is_empty() => Ok("{}".to_string()),
        Value::Object(map) => mapping(map, place.level, place.compact),
    }
}

/// A sequence that is a mapping's value sits at the mapping's own column.
fn sequence_level(place: Place) -> usize {
    if place.in_sequence || place.level == 0 {
        place.level
    } else {
        place.level - 1
    }
}

fn line_start(level: usize) -> String {
    format!("\n{}", " ".repeat(2 * level))
}

fn sequence(items: &[Value], level: usize, compact: bool) -> Result<String, FormatError> {
    let mut text = String::new();
    for (index, item) in items.iter().enumerate() {
        let rendered = node(item, Place { level: level + 1, compact: true, in_sequence: true })?;
        if !compact || index > 0 {
            text.push_str(&line_start(level));
        }
        text.push_str(if rendered.starts_with('\n') { "-" } else { "- " });
        text.push_str(&rendered);
    }
    Ok(text)
}

fn mapping(map: &Map<String, Value>, level: usize, compact: bool) -> Result<String, FormatError> {
    let mut text = String::new();
    for (index, (key, value)) in map.iter().enumerate() {
        if !compact || index > 0 {
            text.push_str(&line_start(level));
        }
        text.push_str(&pair(key, value, level)?);
    }
    Ok(text)
}

/// One `key: value` pair. A key too long for that form is written as an explicit key,
/// `? key` on one line and `: value` on the next.
fn pair(key: &str, value: &Value, level: usize) -> Result<String, FormatError> {
    let rendered_key = render_string(key, 2 * (level + 1), true);
    let explicit = rendered_key.len() > LONGEST_IMPLICIT_KEY;
    let rendered_value = node(value, Place { level: level + 1, compact: explicit, in_sequence: false })?;
    let separator = if rendered_value.starts_with('\n') { ":" } else { ": " };
    if explicit {
        return Ok(format!("? {rendered_key}{}{separator}{rendered_value}", line_start(level)));
    }
    Ok(format!("{rendered_key}{separator}{rendered_value}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use serde_json::json;

    #[test]
    fn a_mapping_is_block_style_with_sequences_at_the_key_column() {
        let value = json!({"title": "A page", "tags": ["B", "A"], "nested": {"start": "1977-06", "count": 3}});
        assert_eq!(
            emit(&value).unwrap(),
            "title: A page\ntags:\n- B\n- A\nnested:\n  start: \"1977-06\"\n  count: 3\n"
        );
    }

    #[test]
    fn nested_collections_follow_the_same_layout() {
        let value = json!({"a": [{"k": 1, "l": [1, 2]}, [3, [4]], "x"], "b": {"c": {"d": []}}, "e": {}});
        assert_eq!(
            emit(&value).unwrap(),
            "a:\n- k: 1\n  l:\n  - 1\n  - 2\n- - 3\n  - - 4\n- x\nb:\n  c:\n    d: []\ne: {}\n"
        );
    }

    #[test]
    fn scalars_and_empty_collections_stand_alone_as_documents() {
        assert_eq!(emit(&json!(null)).unwrap(), "null\n");
        assert_eq!(emit(&json!(true)).unwrap(), "true\n");
        assert_eq!(emit(&json!("plain")).unwrap(), "plain\n");
        assert_eq!(emit(&json!([])).unwrap(), "[]\n");
        assert_eq!(emit(&json!({})).unwrap(), "{}\n");
        assert_eq!(emit(&json!(["a", "b"])).unwrap(), "- a\n- b\n");
        assert_eq!(emit(&json!("l1\nl2\n")).unwrap(), "|\n  l1\n  l2\n");
    }

    #[test]
    fn a_literal_block_sits_under_its_key_and_under_its_item() {
        let value = json!({"note": "l1\nl2\n", "items": ["a\nb"], "last": "x\n\n"});
        assert_eq!(
            emit(&value).unwrap(),
            "note: |\n  l1\n  l2\nitems:\n- |-\n  a\n  b\nlast: |+\n  x\n\n"
        );
    }

    #[test]
    fn a_key_too_long_for_one_line_is_an_explicit_key() {
        let key = "k".repeat(1025);
        let value = json!({ key.clone(): {"a": 1} });
        assert_eq!(emit(&value).unwrap(), format!("? {key}\n: a: 1\n"));
    }

    #[test]
    fn an_integer_beyond_two_to_the_53_refuses_the_whole_document() {
        let error = emit(&json!({"ok": 1, "big": 9007199254740993_u64})).unwrap_err();
        assert!(matches!(error, FormatError::Conversion(_)));
    }

    /// Values that a careless writer changes: each must read back type-exactly, and the
    /// text written from what was read must be the text first written.
    #[test]
    fn written_documents_read_back_as_the_same_data_and_write_again_as_the_same_bytes() {
        let values = [
            json!({"date": "1999-04-01", "time": "12:30", "word": "yes", "sep": "1_000", "n": "42"}),
            json!({"int": 7, "float": 1.0, "small": 1e-7, "big": 1e300, "neg": -0.5, "edge": 9007199254740992_u64}),
            json!({"null": null, "true": true, "yes": 1, "1": "one", "": "empty key", "a b": [null, false]}),
            json!({"text": "l1\nl2", "clip": "l1\n", "keep": "l1\n\n\n", "only": "\n", "lead": "  x\ny"}),
            json!({"tab": "a\tb", "cr": "a\rb", "quote": "\"q\"", "colon": "a: b", "hash": "a #b", "nbsp": "a\u{A0}b"}),
            json!({"deep": [[["x"]], {"m": [{"n": {}}]}, []], "emoji": "🏛️", "dash": "- x", "space": " x "}),
            json!(["...", "---", "~", "", " ", "<<", "0x1F", ".inf", "2001-12-14T21:59:43Z"]),
        ];
        for value in values {
            let written = emit(&value).unwrap();
            let read = parse::yaml(&written).unwrap();
            assert_eq!(read, value, "{written}");
            assert_eq!(emit(&read).unwrap(), written);
        }
    }
}
