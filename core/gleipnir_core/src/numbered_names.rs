//! Numbered-name patterns shared by the Python, Rust and TypeScript style checks.
//!
//! A name is a bad numbered name when its language's numbered pattern matches it
//! and its language's allow regex does not. The allow-list has one definition
//! here; each language adds only what is its own.

use regex::Regex;
use std::sync::LazyLock;

/// Names ending in digits that carry meaning in every language.
const SHARED_ALLOWED: &[&str] = &[
    r"v\d+",
    r"utf_?\d+",
    r"sha\d+",
    r"md\d+",
    r"h\d+",
    r"ipv\d+",
    r"base\d+",
    r"int\d+",
    r"float\d+",
    r"rgb\d*",
    r"level\d+",
    r"phase\d+",
    r"log\d*",
    r"p\d{4,}",
];

const PYTHON_ALLOWED_ADDITIONS: &[&str] = &[];

/// Rust's primitive numeric type names (u8, i32, f64).
const RUST_ALLOWED_ADDITIONS: &[&str] = &[r"u\d+", r"i\d+", r"f\d+"];

const TYPESCRIPT_ALLOWED_ADDITIONS: &[&str] = &[];

/// The allow regex source: the shared alternatives plus one language's additions.
fn allowed_source(additions: &[&str]) -> String {
    let mut alternatives: Vec<&str> = SHARED_ALLOWED.to_vec();
    alternatives.extend_from_slice(additions);
    format!("^({})$", alternatives.join("|"))
}

static PYTHON_ALLOWED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&allowed_source(PYTHON_ALLOWED_ADDITIONS)).unwrap());

static RUST_ALLOWED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&allowed_source(RUST_ALLOWED_ADDITIONS)).unwrap());

static TYPESCRIPT_ALLOWED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&allowed_source(TYPESCRIPT_ALLOWED_ADDITIONS)).unwrap());

static PYTHON_NUMBERED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z_]*\d+$").unwrap());

static RUST_NUMBERED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z_]*\d+$").unwrap());

// TypeScript differs: its identifiers are camelCase, so the body admits uppercase letters.
static TYPESCRIPT_NUMBERED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-zA-Z_]*\d+$").unwrap());

/// Numbered names allowed in Python.
pub(crate) fn python_allowed() -> &'static Regex {
    &PYTHON_ALLOWED
}

/// Numbered names allowed in Rust.
pub(crate) fn rust_allowed() -> &'static Regex {
    &RUST_ALLOWED
}

/// Numbered names allowed in TypeScript.
pub(crate) fn typescript_allowed() -> &'static Regex {
    &TYPESCRIPT_ALLOWED
}

/// What a numbered name looks like in Python.
pub(crate) fn python_numbered() -> &'static Regex {
    &PYTHON_NUMBERED
}

/// What a numbered name looks like in Rust.
pub(crate) fn rust_numbered() -> &'static Regex {
    &RUST_NUMBERED
}

/// What a numbered name looks like in TypeScript.
pub(crate) fn typescript_numbered() -> &'static Regex {
    &TYPESCRIPT_NUMBERED
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One or more names per shared alternative, in the order of SHARED_ALLOWED.
    const SHARED_ALLOWED_NAMES: &[&str] = &[
        "v2", "utf8", "utf_8", "sha256", "md5", "h1", "ipv4", "base64", "int32", "float64",
        "rgb", "rgb8", "level3", "phase2", "log", "log2", "p1234",
    ];

    #[test]
    fn every_shared_alternative_has_a_name_under_test() {
        for alternative in SHARED_ALLOWED {
            let single = Regex::new(&format!("^({alternative})$")).unwrap();
            assert!(
                SHARED_ALLOWED_NAMES.iter().any(|name| single.is_match(name)),
                "no test name exercises {alternative}"
            );
        }
    }

    #[test]
    fn shared_names_allowed_in_all_three_languages() {
        for name in SHARED_ALLOWED_NAMES {
            assert!(python_allowed().is_match(name), "{name} allowed in Python");
            assert!(rust_allowed().is_match(name), "{name} allowed in Rust");
            assert!(typescript_allowed().is_match(name), "{name} allowed in TypeScript");
        }
    }

    #[test]
    fn primitive_type_names_allowed_only_in_rust() {
        for name in ["u8", "i32", "f64"] {
            assert!(rust_allowed().is_match(name), "{name} allowed in Rust");
            assert!(!python_allowed().is_match(name), "{name} not allowed in Python");
            assert!(!typescript_allowed().is_match(name), "{name} not allowed in TypeScript");
        }
    }

    #[test]
    fn plain_numbered_name_flagged_in_all_three_languages() {
        let name = "thing2";
        assert!(python_numbered().is_match(name) && !python_allowed().is_match(name));
        assert!(rust_numbered().is_match(name) && !rust_allowed().is_match(name));
        assert!(typescript_numbered().is_match(name) && !typescript_allowed().is_match(name));
    }

    #[test]
    fn a_numbered_step_is_flagged_in_all_three_languages() {
        // A numbered step names a sequence. The name is read as an instruction
        // long after the order it claims has changed.
        for name in ["step1", "step_1", "step12"] {
            assert!(python_numbered().is_match(name) && !python_allowed().is_match(name), "{name} in Python");
            assert!(rust_numbered().is_match(name) && !rust_allowed().is_match(name), "{name} in Rust");
            assert!(typescript_numbered().is_match(name) && !typescript_allowed().is_match(name), "{name} in TypeScript");
        }
    }

    #[test]
    fn camel_case_numbered_name_is_numbered_only_in_typescript() {
        let name = "userData2";
        assert!(typescript_numbered().is_match(name));
        assert!(!python_numbered().is_match(name));
        assert!(!rust_numbered().is_match(name));
    }

    #[test]
    fn name_without_trailing_digits_is_not_numbered() {
        let name = "thing";
        assert!(!python_numbered().is_match(name));
        assert!(!rust_numbered().is_match(name));
        assert!(!typescript_numbered().is_match(name));
    }
}
