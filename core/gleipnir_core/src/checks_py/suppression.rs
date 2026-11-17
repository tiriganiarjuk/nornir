//! Suppression comment detection.
//!
//! Check: no_suppression_comments.

use regex::Regex;
use std::sync::LazyLock;

use crate::structures::{CheckConfig, ParsedSource, Severity, Violation};

fn violation(line: usize, message: String) -> Violation {
    Violation {
        line,
        check_name: String::new(),
        severity: Severity::Blocked,
        message,
        detail: String::new(),
        signal: String::new(),
        direction: String::new(),
        canary: String::new(),
    }
}

// Patterns and descriptions constructed to avoid self-triggering.
struct ForbiddenPattern {
    regex: Regex,
    description: String,
}

static FORBIDDEN_PATTERNS: LazyLock<Vec<ForbiddenPattern>> = LazyLock::new(|| {
    let build_desc = |prefix: &str, suffix: &str| format!("# {prefix}: {suffix}");
    vec![
        // Blanket: alone, before another comment, or before words explaining it.
        ForbiddenPattern {
            regex: Regex::new(r"(?i)#\s*type:\s*ignore(?:\s*$|\s*#|\s+[^\s\[])").unwrap(),
            description: build_desc("type", "ignore (blanket suppression)"),
        },
        ForbiddenPattern {
            regex: Regex::new(r"(?i)#\s*type:\s*ignore\s*\[").unwrap(),
            description: build_desc("type", "ignore[...] (use pyright syntax instead)"),
        },
        ForbiddenPattern {
            regex: Regex::new(r"(?i)#\s*noqa(?:\s*$|\s*#|\s*:)").unwrap(),
            description: "# noqa (ruff/flake8 suppression)".to_string(),
        },
        ForbiddenPattern {
            regex: Regex::new(r"(?i)#\s*pylint:\s*disable").unwrap(),
            description: build_desc("pylint", "disable (pylint suppression)"),
        },
        ForbiddenPattern {
            regex: Regex::new(r"(?i)#\s*pyright:\s*ignore\s*(?:$|#)").unwrap(),
            description: build_desc("pyright", "ignore (blanket - must specify error code)"),
        },
        // File-wide: one comment that turns a tool down for every line of the file.
        ForbiddenPattern {
            regex: Regex::new(r"(?i)#\s*(?:ruff|flake8)\s*:\s*noqa").unwrap(),
            description: "file-wide noqa (ruff/flake8 suppression for the whole file)".to_string(),
        },
        ForbiddenPattern {
            regex: Regex::new(r"(?i)#\s*pyright:\s*basic\b").unwrap(),
            description: build_desc("pyright", "basic (type checking lowered for the whole file)"),
        },
        ForbiddenPattern {
            regex: Regex::new(r"(?i)#\s*pyright:[^#]*\breport\w+\s*=\s*(?:false|none)\b").unwrap(),
            description: build_desc("pyright", "report...=false (a rule switched off for the whole file)"),
        },
        ForbiddenPattern {
            regex: Regex::new(r"(?i)#\s*mypy:\s*(?:ignore-errors|disable-error-code)").unwrap(),
            description: build_desc("mypy", "ignore-errors / disable-error-code (checking switched off for the whole file)"),
        },
    ]
});

static PYRIGHT_IGNORE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)#\s*pyright:\s*ignore\s*\[([^\]]+)\]").unwrap());

fn pyright_msg_prefix() -> String {
    format!("# {}: {}", "pyright", "ignore")
}

pub fn check_no_suppression_comments(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let prefix = pyright_msg_prefix();

    for (line_num, line) in source.lines.iter().enumerate() {
        let line_num = line_num + 1;

        let mut found_forbidden = false;
        for fp in FORBIDDEN_PATTERNS.iter() {
            if fp.regex.is_match(line) {
                violations.push(violation(line_num, fp.description.clone()));
                found_forbidden = true;
                break;
            }
        }

        if found_forbidden {
            continue;
        }

        // Pyright ignore with specific codes — always a violation (fully strict)
        if let Some(caps) = PYRIGHT_IGNORE_RE.captures(line) {
            let codes_str = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            violations.push(violation(
                line_num,
                format!("{prefix}[{codes_str}] - suppression comment"),
            ));
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::build_parsed_source;
    use crate::structures::FileKind;

    fn parse(code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source("/test/file.py", source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside, &crate::STATISTICS)
    }

    #[test]
    fn blanket_type_ignore_caught() {
        let comment = format!("x = 1  # {}: {}", "type", "ignore");
        let parsed = parse(&comment);
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("blanket suppression"));
    }

    #[test]
    fn noqa_caught() {
        let comment = format!("x = 1  # {}", "noqa");
        let parsed = parse(&comment);
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("noqa"));
    }

    #[test]
    fn pylint_disable_caught() {
        let comment = format!("x = 1  # {}: {}", "pylint", "disable=C0301");
        let parsed = parse(&comment);
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("pylint"));
    }

    #[test]
    fn pyright_ignore_with_code_caught() {
        let comment = format!("x = 1  # {}: {}[reportGeneralIssue]", "pyright", "ignore");
        let parsed = parse(&comment);
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("suppression comment"));
    }

    #[test]
    fn blanket_pyright_ignore_caught() {
        let comment = format!("x = 1  # {}: {}", "pyright", "ignore");
        let parsed = parse(&comment);
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("blanket"));
    }

    #[test]
    fn file_wide_suppressions_caught() {
        // The edge: a comment that turns a tool down for the whole file was not looked for.
        for comment in [
            format!("# {}: {}", "ruff", "noqa"),
            format!("# {}: {}: E501", "ruff", "noqa"),
            format!("# {}: {}", "flake8", "noqa"),
            format!("# {}: {}", "pyright", "basic"),
            format!("# {}: {}=false", "pyright", "reportMissingImports"),
            format!("# {}: {}=none, reportAny=false", "pyright", "reportUnknownMemberType"),
            format!("# {}: {}", "mypy", "ignore-errors"),
            format!("# {}: {}=attr-defined", "mypy", "disable-error-code"),
        ] {
            let parsed = parse(&format!("{comment}\nx = 1\n"));
            assert_eq!(check_no_suppression_comments(&parsed, &default_config()).len(), 1, "{comment}");
        }
    }

    #[test]
    fn type_ignore_followed_by_words_caught() {
        // The edge: a blanket ignore with an explanation after it slipped both patterns.
        let comment = format!("x = 1  # {}: {} because the stub is wrong", "type", "ignore");
        assert_eq!(check_no_suppression_comments(&parse(&comment), &default_config()).len(), 1);
    }

    #[test]
    fn directives_that_tighten_or_name_a_tool_are_ok() {
        for line in [
            format!("# {}: {}", "pyright", "strict"),
            format!("# {}: {}=error", "pyright", "reportAny"),
            "# the ruff docs say noqa is discouraged".to_string(),
            "# types: ignored here are listed below".to_string(),
        ] {
            let parsed = parse(&format!("{line}\nx = 1\n"));
            assert!(check_no_suppression_comments(&parsed, &default_config()).is_empty(), "{line}");
        }
    }

    #[test]
    fn clean_code_ok() {
        let parsed = parse("x = 1  # normal comment\n");
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert!(violations.is_empty());
    }
}
