//! TOON and XML written only when what is written reads back as the data handed over.
//!
//! Neither format carries every JSON value: TOON has no integers beyond 64 signed bits and
//! writes large floats as other things; XML has no types, drops surrounding whitespace and
//! cannot tell an empty element from an empty string. Each function here serializes the
//! data, parses its own output, and refuses with the path of the first value that came
//! back different. The raw serializers stay in [`crate::serialize`] for presentation.

use error_core::FormatError;
use serde_json::{Number, Value};

use crate::{parse, serialize};

/// TOON for `value`, refused when TOON would hand back different data.
pub fn to_toon_verified(value: &Value) -> Result<String, FormatError> {
    let text = serialize::to_toon(value)?;
    verify("TOON", value, parse::toon(&text), text)
}

/// XML for `value`, refused when XML would hand back different data.
pub fn to_xml_verified(value: &Value) -> Result<String, FormatError> {
    let text = serialize::to_xml(value)?;
    verify("XML", value, parse::xml(&text), text)
}

fn verify(format: &str, value: &Value, read: Result<Value, FormatError>, text: String) -> Result<String, FormatError> {
    let read = read.map_err(|error| {
        FormatError::Conversion(format!("{format} cannot carry this data: what it writes does not read back ({error})"))
    })?;
    match first_difference(value, &read, "") {
        None => Ok(text),
        Some(path) => Err(FormatError::Conversion(format!(
            "{format} cannot carry the value at {}: it reads back as a different value",
            if path.is_empty() { "the root" } else { path.as_str() }
        ))),
    }
}

/// The path of the first place `read` differs from `written`; `None` when they are the
/// same data. Key order is not data, and a number is the same number whether it is held
/// as an integer or as a float of exactly that value.
pub(crate) fn first_difference(written: &Value, read: &Value, path: &str) -> Option<String> {
    match (written, read) {
        (Value::Object(left), Value::Object(right)) => {
            if left.len() != right.len() {
                return Some(path.to_string());
            }
            left.iter().find_map(|(key, value)| {
                let below = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                match right.get(key) {
                    Some(other) => first_difference(value, other, &below),
                    None => Some(below),
                }
            })
        }
        (Value::Array(left), Value::Array(right)) => {
            if left.len() != right.len() {
                return Some(path.to_string());
            }
            left.iter()
                .zip(right)
                .enumerate()
                .find_map(|(index, (value, other))| first_difference(value, other, &format!("{path}[{index}]")))
        }
        (Value::Number(left), Value::Number(right)) => (!same_number(left, right)).then(|| path.to_string()),
        _ => (written != read).then(|| path.to_string()),
    }
}

fn same_number(left: &Number, right: &Number) -> bool {
    match (exact_integer(left), exact_integer(right)) {
        (Some(one), Some(other)) => one == other,
        (None, None) => left.as_f64() == right.as_f64(),
        _ => false,
    }
}

/// The number as an exact integer, when it is one: an integer, or a float with no
/// fraction that a 128-bit integer holds exactly.
fn exact_integer(number: &Number) -> Option<i128> {
    if let Some(integer) = number.as_i64() {
        return Some(i128::from(integer));
    }
    if let Some(integer) = number.as_u64() {
        return Some(i128::from(integer));
    }
    let float = number.as_f64()?;
    let whole = float.fract() == 0.0 && float.abs() < 1e37;
    whole.then(|| float as i128)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn toon_writes_data_it_hands_back() {
        let value = json!({"name": "x", "count": 3, "ratio": 0.5, "items": [{"a": 1, "b": "y"}, {"a": 2, "b": "z"}]});
        let text = to_toon_verified(&value).unwrap();
        assert_eq!(parse::toon(&text).unwrap(), value);
    }

    #[test]
    fn toon_refuses_integers_and_floats_it_would_change() {
        for value in [
            json!({"v": 9223372036854775808_u64}),
            json!({"v": 18446744073709551615_u64}),
            json!({"v": 9223372036854775808.0}),
            json!({"v": 1e300}),
            json!({"v": 1e21}),
            json!({"v": [{}]}),
        ] {
            let error = to_toon_verified(&value).unwrap_err();
            assert!(matches!(error, FormatError::Conversion(_)), "{value}");
            assert!(error.to_string().contains("TOON cannot carry"), "{error}");
        }
    }

    #[test]
    fn xml_writes_data_it_hands_back() {
        let value = json!({"root": {"@id": "7", "name": "x", "item": ["a", "b"], "empty": {}, "deep": {"#text": "t", "@k": "v"}}});
        let text = to_xml_verified(&value).unwrap();
        assert_eq!(parse::xml(&text).unwrap(), value);
    }

    #[test]
    fn xml_refuses_what_it_cannot_tell_apart() {
        for value in [
            json!({"root": {"n": 1}}),
            json!({"root": {"flag": true}}),
            json!({"root": {"gone": null}}),
            json!({"root": {"blank": ""}}),
            json!({"root": {"padded": " x "}}),
            json!({"root": {"list": []}}),
            json!({"root": {"single": ["only"]}}),
            json!({"root": {"nested": [["a"], ["b"]]}}),
            json!({"root": {"@id": 7}}),
            json!({"root": {"#text": 5, "a": "x"}}),
        ] {
            let error = to_xml_verified(&value).unwrap_err();
            assert!(error.to_string().contains("XML cannot carry"), "{value}: {error}");
        }
    }

    #[test]
    fn the_refusal_names_where_the_data_changed() {
        let error = to_xml_verified(&json!({"root": {"ok": "x", "bad": {"n": 1}}})).unwrap_err();
        assert!(error.to_string().contains("root.bad.n"), "{error}");
    }

    #[test]
    fn a_number_is_the_same_number_as_an_integer_or_a_float() {
        assert_eq!(first_difference(&json!(1.0), &json!(1), ""), None);
        assert_eq!(first_difference(&json!(1e16), &json!(10000000000000000_u64), ""), None);
        assert_eq!(first_difference(&json!(9223372036854775808.0), &json!(9223372036854775807_i64), ""), Some(String::new()));
        assert_eq!(first_difference(&json!(0.5), &json!(0.5), ""), None);
        assert_eq!(first_difference(&json!({"a": 1, "b": 2}), &json!({"b": 2, "a": 1}), ""), None);
        assert_eq!(first_difference(&json!({"a": [1, 2]}), &json!({"a": [1, 3]}), ""), Some("a[1]".to_string()));
    }
}
