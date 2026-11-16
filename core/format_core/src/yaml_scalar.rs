//! One YAML scalar, rendered so that every mainstream reader types it the same way.
//!
//! A string is written plain only when YAML 1.1 (PyYAML), YAML 1.2 core (serde_yaml) and
//! js-yaml (3 and 4, every schema) all read the plain form as that same string. Any other
//! string is double-quoted, or written as a literal block when it holds line breaks.
//! Floats always carry a `.` and a signed exponent; integers are written only while a
//! JavaScript reader can hold them exactly.
//!
//! The style is js-yaml's own (`dump` with `quotingType: '"'`, `lineWidth: -1`), so a file
//! js-yaml wrote is, apart from the stricter quoting, the file this writes.

use error_core::FormatError;
use serde_json::Number;

/// The largest magnitude an integer may have and still be exact in a JavaScript reader.
const LARGEST_EXACT_INTEGER: u64 = 1 << 53;

/// Words some YAML reader takes for null or a boolean, in any letter case.
const RESERVED_WORDS: [&str; 9] = ["null", "true", "false", "yes", "no", "on", "off", "y", "n"];

/// How a string is written.
#[derive(Debug, PartialEq, Eq)]
enum ScalarStyle {
    Plain,
    DoubleQuoted,
    Literal,
}

/// A string as a YAML scalar. `indent` is the column a literal block's lines start at;
/// a key is always written on one line.
pub(crate) fn render_string(text: &str, indent: usize, is_key: bool) -> String {
    match choose_style(text, is_key) {
        ScalarStyle::Plain => text.to_string(),
        ScalarStyle::DoubleQuoted => format!("\"{}\"", escape_double_quoted(text)),
        ScalarStyle::Literal => format!("|{}{}", block_header(text), indent_block_lines(text, indent)),
    }
}

/// A number as a YAML scalar. An integer beyond ±2^53 is refused: a JavaScript reader
/// would round it.
pub(crate) fn render_number(number: &Number) -> Result<String, FormatError> {
    if let Some(integer) = number.as_i64() {
        return exact_integer(integer.unsigned_abs(), number);
    }
    if let Some(integer) = number.as_u64() {
        return exact_integer(integer, number);
    }
    match number.as_f64() {
        Some(float) => Ok(render_float(float)),
        None => Err(FormatError::Conversion(format!("the number {number} cannot be written as YAML"))),
    }
}

fn exact_integer(magnitude: u64, number: &Number) -> Result<String, FormatError> {
    if magnitude > LARGEST_EXACT_INTEGER {
        return Err(FormatError::Conversion(format!(
            "the integer {number} is beyond ±9007199254740992 (2^53), which JavaScript YAML readers \
             round; hand it over as a string"
        )));
    }
    Ok(number.to_string())
}

/// The shortest text that reads back as the same float, with a `.` in the mantissa and a
/// sign on the exponent: the one form YAML 1.1 and YAML 1.2 readers both take for a float.
fn render_float(float: f64) -> String {
    let shortest = format!("{float:?}");
    let Some((mantissa, exponent)) = shortest.split_once('e') else {
        return shortest;
    };
    let dotted = if mantissa.contains('.') { mantissa.to_string() } else { format!("{mantissa}.0") };
    let sign = if exponent.starts_with('-') { "" } else { "+" };
    format!("{dotted}e{sign}{exponent}")
}

fn choose_style(text: &str, is_key: bool) -> ScalarStyle {
    let mut has_line_break = false;
    let mut plain = text.chars().next().is_some_and(is_plain_safe_first)
        && text.chars().last().is_some_and(is_plain_safe_last);
    let mut previous = None;
    for character in text.chars() {
        if character == '\n' && !is_key {
            has_line_break = true;
        } else if !is_printable(character) {
            return ScalarStyle::DoubleQuoted;
        }
        plain = plain && is_plain_safe(character, previous);
        previous = Some(character);
    }
    if has_line_break {
        return ScalarStyle::Literal;
    }
    if plain && !reads_as_non_string(text) {
        return ScalarStyle::Plain;
    }
    ScalarStyle::DoubleQuoted
}

/// Whether some mainstream YAML reader takes the plain form of `text` for something other
/// than this string. It errs toward true: a needless quote changes no data.
pub(crate) fn reads_as_non_string(text: &str) -> bool {
    is_reserved_word(text)
        || is_number_like(text)
        || is_special_float(text)
        || starts_with_date(text)
        || text.starts_with("...")
        || text.starts_with("---")
}

fn is_reserved_word(text: &str) -> bool {
    text == "~" || text == "<<" || RESERVED_WORDS.iter().any(|word| text.eq_ignore_ascii_case(word))
}

fn without_sign(text: &str) -> &str {
    text.strip_prefix(['+', '-']).unwrap_or(text)
}

/// Every form a reader takes for an integer or a float — decimal, hex, octal, binary,
/// `_` separators, exponents, base 60 — opens with a digit, a `.` or a `_` and is drawn
/// from these characters. A lone `.` is among them: one reader fails on it.
fn is_number_like(text: &str) -> bool {
    let body = without_sign(text);
    let leads = body.chars().next().is_some_and(|first| first.is_ascii_digit() || first == '.' || first == '_');
    leads && body.chars().all(is_number_character)
}

fn is_number_character(character: char) -> bool {
    character.is_ascii_hexdigit() || matches!(character, 'x' | 'X' | 'o' | 'O' | '_' | '.' | ':' | '+' | '-')
}

fn is_special_float(text: &str) -> bool {
    let body = without_sign(text);
    body.eq_ignore_ascii_case(".inf") || body.eq_ignore_ascii_case(".nan")
}

/// `YYYY-M-D` at the start: the opening of every timestamp form.
fn starts_with_date(text: &str) -> bool {
    let mut parts = text.splitn(3, '-');
    let (Some(year), Some(month), Some(rest)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let all_digits = |part: &str| !part.is_empty() && part.chars().all(|character| character.is_ascii_digit());
    year.len() == 4
        && all_digits(year)
        && month.len() <= 2
        && all_digits(month)
        && rest.chars().next().is_some_and(|first| first.is_ascii_digit())
}

/// Characters written as themselves; every other one is escaped inside double quotes.
fn is_printable(character: char) -> bool {
    matches!(character,
        '\u{20}'..='\u{7E}'
        | '\u{A1}'..='\u{2027}'
        | '\u{202A}'..='\u{D7FF}'
        | '\u{E000}'..='\u{FEFE}'
        | '\u{FF00}'..='\u{FFFD}'
        | '\u{10000}'..='\u{10FFFF}')
}

fn is_plain_safe_first(character: char) -> bool {
    is_printable(character)
        && !matches!(
            character,
            ' ' | '-' | '?' | ':' | ',' | '[' | ']' | '{' | '}' | '#' | '&' | '*' | '!' | '|' | '=' | '>' | '\''
                | '"' | '%' | '@' | '`'
        )
}

fn is_plain_safe_last(character: char) -> bool {
    character != ' ' && character != ':'
}

/// Whether `character` may stand in a plain scalar after `previous`: a `#` only directly
/// after a non-space, and nothing but a non-space after a `:`.
fn is_plain_safe(character: char, previous: Option<char>) -> bool {
    let after_colon = previous == Some(':');
    let after_non_space = previous.is_some_and(|before| is_printable(before) && before != ' ');
    if character == '#' {
        return after_non_space;
    }
    is_printable(character) && !(after_colon && character == ' ')
}

fn escape_double_quoted(text: &str) -> String {
    text.chars().map(escaped_character).collect()
}

fn escaped_character(character: char) -> String {
    match character {
        '\0' => "\\0".to_string(),
        '\u{07}' => "\\a".to_string(),
        '\u{08}' => "\\b".to_string(),
        '\t' => "\\t".to_string(),
        '\n' => "\\n".to_string(),
        '\u{0B}' => "\\v".to_string(),
        '\u{0C}' => "\\f".to_string(),
        '\r' => "\\r".to_string(),
        '\u{1B}' => "\\e".to_string(),
        '"' => "\\\"".to_string(),
        '\\' => "\\\\".to_string(),
        '\u{85}' => "\\N".to_string(),
        '\u{A0}' => "\\_".to_string(),
        '\u{2028}' => "\\L".to_string(),
        '\u{2029}' => "\\P".to_string(),
        printable if is_printable(printable) => printable.to_string(),
        other => hex_escape(other),
    }
}

fn hex_escape(character: char) -> String {
    let code = character as u32;
    if code <= 0xFF {
        format!("\\x{code:02X}")
    } else if code <= 0xFFFF {
        format!("\\u{code:04X}")
    } else {
        format!("\\U{code:08X}")
    }
}

/// What follows the `|`: an indentation indicator when the text opens with a space, and
/// the chomping indicator that keeps the text's own trailing line breaks.
fn block_header(text: &str) -> String {
    let indicator = if text.trim_start_matches('\n').starts_with(' ') { "2" } else { "" };
    let clip = text.ends_with('\n');
    let keep = clip && (text.ends_with("\n\n") || text == "\n");
    let chomp = if keep { "+" } else if clip { "" } else { "-" };
    format!("{indicator}{chomp}\n")
}

/// The block's lines, each non-empty one indented; the last line break is left to the
/// document, which ends every node with one.
fn indent_block_lines(text: &str, indent: usize) -> String {
    let margin = " ".repeat(indent);
    let indented: String = text
        .split_inclusive('\n')
        .map(|line| if line == "\n" { line.to_string() } else { format!("{margin}{line}") })
        .collect();
    indented.strip_suffix('\n').unwrap_or(&indented).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value_text(text: &str) -> String {
        render_string(text, 2, false)
    }

    #[test]
    fn an_ordinary_string_is_plain() {
        for text in ["Alaska", "Arctic Footage — Igloolik 1958", "a:b", "it's", "emoji 📜", "a#b", "x, [y]"] {
            assert_eq!(value_text(text), text);
        }
    }

    #[test]
    fn a_string_some_reader_would_mistype_is_quoted() {
        for text in [
            "", "null", "Null", "~", "true", "FALSE", "yes", "No", "on", "OFF", "y", "N", "<<", "1999-04-01",
            "2001-12-14 21:59:43", "2001-12-14T21:59:43Z", "12:30", "1:20", "12:30:45", "1_000", "0x1F", "0o17",
            "0b101", "012", "42", "-7", "+3", "1.5", ".5", "1e3", "1e-7", "._1", ".", "+.", ".inf", "-.INF", ".NaN", "1971",
            "1977-06", "...", "---",
        ] {
            assert_eq!(value_text(text), format!("\"{text}\""), "{text}");
        }
    }

    #[test]
    fn a_string_yaml_syntax_would_break_is_quoted() {
        for text in [" lead", "trail ", "key: value", "a #b", "- item", "? q", "[x]", "{x}", "#c", "&a", "*a",
            "!t", "|", ">", "'q'", "%d", "@h", "`c`", "end:"]
        {
            assert_eq!(value_text(text), format!("\"{text}\""), "{text}");
        }
    }

    #[test]
    fn characters_that_are_not_printable_are_escaped() {
        assert_eq!(value_text("a\tb"), "\"a\\tb\"");
        assert_eq!(value_text("say \"hi\""), "say \"hi\"");
        assert_eq!(value_text("\"hi\""), "\"\\\"hi\\\"\"");
        assert_eq!(value_text("back\\slash: x"), "\"back\\\\slash: x\"");
        assert_eq!(value_text("a\r\nb"), "\"a\\r\\nb\"");
        assert_eq!(value_text("nul\0"), "\"nul\\0\"");
        assert_eq!(value_text("\u{7F}"), "\"\\x7F\"");
        assert_eq!(value_text("\u{FEFF}"), "\"\\uFEFF\"");
    }

    #[test]
    fn a_string_with_line_breaks_is_a_literal_block_that_keeps_them() {
        assert_eq!(value_text("l1\nl2"), "|-\n  l1\n  l2");
        assert_eq!(value_text("l1\n"), "|\n  l1");
        assert_eq!(value_text("l1\n\n"), "|+\n  l1\n");
        assert_eq!(value_text("\n"), "|+\n");
        assert_eq!(value_text(" lead\nx"), "|2-\n   lead\n  x");
        assert_eq!(value_text("a\n\nb\n"), "|\n  a\n\n  b");
    }

    #[test]
    fn a_key_is_always_one_line() {
        assert_eq!(render_string("a\nb", 2, true), "\"a\\nb\"");
        assert_eq!(render_string("yes", 2, true), "\"yes\"");
        assert_eq!(render_string("occurrence/effective", 2, true), "occurrence/effective");
    }

    fn number_text(json: &str) -> Result<String, FormatError> {
        let number: Number = serde_json::from_str(json).unwrap();
        render_number(&number)
    }

    #[test]
    fn an_integer_a_javascript_reader_holds_exactly_is_written() {
        assert_eq!(number_text("0").unwrap(), "0");
        assert_eq!(number_text("-42").unwrap(), "-42");
        assert_eq!(number_text("9007199254740992").unwrap(), "9007199254740992");
        assert_eq!(number_text("-9007199254740992").unwrap(), "-9007199254740992");
    }

    #[test]
    fn an_integer_beyond_two_to_the_53_is_refused() {
        for json in ["9007199254740993", "-9007199254740993", "9223372036854775807", "18446744073709551615"] {
            let error = number_text(json).unwrap_err();
            assert!(matches!(error, FormatError::Conversion(_)), "{json}");
            assert!(error.to_string().contains("2^53"), "{error}");
        }
    }

    #[test]
    fn a_float_carries_a_dot_and_a_signed_exponent() {
        assert_eq!(number_text("1.0").unwrap(), "1.0");
        assert_eq!(number_text("-0.0").unwrap(), "-0.0");
        assert_eq!(number_text("0.1").unwrap(), "0.1");
        assert_eq!(number_text("1e-7").unwrap(), "1.0e-7");
        assert_eq!(number_text("1e16").unwrap(), "1.0e+16");
        assert_eq!(number_text("1e300").unwrap(), "1.0e+300");
        assert_eq!(number_text("1.5e-10").unwrap(), "1.5e-10");
        assert_eq!(number_text("1.5e20").unwrap(), "1.5e+20");
    }
}
