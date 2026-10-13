//! Gleipnir: guardrail checks for Python, Rust, and TypeScript/Svelte source files.
//!
//! Pure computation library. No I/O — caller provides source bytes,
//! library returns violations.

pub mod checks_py;
pub mod checks_rs;
pub mod checks_svelte;
pub mod checks_ts;
pub mod classify;
pub mod matrix;
pub mod parsing;
pub mod structures;

use std::collections::HashMap;
use std::sync::LazyLock;

pub use structures::{
    CheckConfig, CheckMessages, FileKind, Level, ParsedSource, Severity, StatisticsToml,
    V2Classification, Violation, Zone,
};

// Embedded messages, parsed once on first access.
static MESSAGES_TOML: &str = include_str!("../gleipnir_messages.toml");
static STATISTICS_TOML: &str = include_str!("../gleipnir_statistics.toml");

pub static STATISTICS: LazyLock<StatisticsToml> = LazyLock::new(|| {
    toml::from_str(STATISTICS_TOML).expect("gleipnir_statistics.toml parse error")
});

static MESSAGES: LazyLock<HashMap<String, CheckMessages>> = LazyLock::new(|| {
    toml::from_str(MESSAGES_TOML).expect("gleipnir_messages.toml parse error")
});

static DEFAULT_MESSAGES: CheckMessages = CheckMessages {
    detail: String::new(),
    signal: String::new(),
    direction: String::new(),
    canary: String::new(),
};

/// Get the static message fields for a check by name.
fn messages(check_name: &str) -> &'static CheckMessages {
    MESSAGES.get(check_name).unwrap_or(&DEFAULT_MESSAGES)
}

/// Stamp a single violation with check metadata.
fn stamp(viol: &mut Violation, name: &str, severity: Severity, msgs: &CheckMessages) {
    if viol.check_name.is_empty() {
        viol.check_name = name.to_string();
    }
    viol.severity = severity;
    if viol.detail.is_empty() {
        viol.detail.clone_from(&msgs.detail);
    }
    if viol.signal.is_empty() {
        viol.signal.clone_from(&msgs.signal);
    }
    if viol.direction.is_empty() {
        viol.direction.clone_from(&msgs.direction);
    }
    if viol.canary.is_empty() {
        viol.canary.clone_from(&msgs.canary);
    }
}

/// Stamp violations with check metadata and collect into output vec.
///
/// Each violation gets the check name, severity, and message fields
/// (detail, signal, direction, canary) from the embedded messages TOML.
fn stamp_and_collect(
    name: &str,
    severity: Severity,
    mut check_violations: Vec<Violation>,
    output: &mut Vec<Violation>,
) {
    let default_msgs = messages(name);
    for viol in &mut check_violations {
        // If the check set its own check_name, use that for message lookup
        let msgs = if !viol.check_name.is_empty() && viol.check_name != name {
            messages(&viol.check_name)
        } else {
            default_msgs
        };
        stamp(viol, name, severity, msgs);
    }
    output.extend(check_violations);
}

/// Run all applicable gleipnir checks on a Python source file.
///
/// Classifies the file, selects checks from the matrix, parses with
/// tree-sitter, runs checks, returns violations.
pub fn run_checks(
    file_path: &str,
    source: &[u8],
) -> Vec<Violation> {
    let first_line = source
        .split(|&b| b == b'\n')
        .next()
        .and_then(|l| std::str::from_utf8(l).ok())
        .unwrap_or("");

    let kind = classify::classify_file(file_path, first_line);
    let config = CheckConfig::for_kind(kind, &STATISTICS);
    let entries = matrix::checks_for_kind(kind);

    let parsed = match parsing::build_parsed_source(file_path, source) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let mut violations = Vec::new();
    for entry in &entries {
        let check_violations = (entry.check_fn)(&parsed, &config);
        stamp_and_collect(entry.name, entry.severity, check_violations, &mut violations);
    }
    violations
}

/// Run v2 zone architecture checks on a Python source file.
///
/// Uses the two-axis classification (level + zone) to select checks.
/// Scripts and tests fall back to v1 rules.
pub fn run_checks_v2(
    file_path: &str,
    source: &[u8],
) -> Vec<Violation> {
    let first_line = source
        .split(|&b| b == b'\n')
        .next()
        .and_then(|l| std::str::from_utf8(l).ok())
        .unwrap_or("");

    // Scripts and tests use v1 rules regardless
    if first_line.starts_with("#!/usr/bin/env -S uv run") {
        return run_checks(file_path, source);
    }
    let filename = file_path.rsplit('/').next().unwrap_or(file_path);
    if filename.contains("test") {
        return run_checks(file_path, source);
    }

    let classification = classify::classify_file_v2(file_path);
    let config = CheckConfig::for_v2(classification.level, classification.zone, &STATISTICS);
    let entries = matrix::checks_for_v2(&classification);

    let parsed = match parsing::build_parsed_source(file_path, source) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let mut violations = Vec::new();
    for entry in &entries {
        let check_violations = (entry.check_fn)(&parsed, &config);
        stamp_and_collect(entry.name, entry.severity, check_violations, &mut violations);
    }
    violations
}

/// Run all applicable gleipnir checks on a Rust source file.
///
/// Parses with tree-sitter-rust, runs Rust-specific checks, returns violations.
/// Skips test modules and #[test] functions for panic-on-failure checks.
pub fn run_checks_rust(file_path: &str, source: &[u8]) -> Vec<Violation> {
    let parsed = match parsing::build_parsed_source_rust(file_path, source) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let config = CheckConfig::for_rust(&STATISTICS);

    type CheckFn = fn(&structures::ParsedSource, &CheckConfig) -> Vec<Violation>;
    let rust_checks: &[(&str, Severity, CheckFn)] = &[
        // PROHIBITED
        ("no_unwrap", Severity::Error, checks_rs::prohibited::check_no_unwrap),
        ("no_println", Severity::Error, checks_rs::prohibited::check_no_println),
        ("no_clone_spam", Severity::Warning, checks_rs::prohibited::check_no_clone_spam),
        ("no_string_abuse", Severity::Warning, checks_rs::prohibited::check_no_string_abuse),
        ("no_pub_overuse", Severity::Warning, checks_rs::prohibited::check_no_pub_overuse),
        // SUPPRESSION
        ("no_suppression_comments_rs", Severity::Error, checks_rs::suppression::check_no_suppression_comments),
        // STYLE
        ("function_length_rs", Severity::Warning, checks_rs::style::check_function_length),
        ("param_count_rs", Severity::Warning, checks_rs::style::check_param_count),
        ("nesting_depth_rs", Severity::Warning, checks_rs::style::check_nesting_depth),
        ("no_underscore_prefix_rs", Severity::Warning, checks_rs::style::check_no_underscore_prefix),
        ("no_single_letter_names_rs", Severity::Warning, checks_rs::style::check_no_single_letter_names),
        ("no_numbered_suffixes_rs", Severity::Warning, checks_rs::style::check_no_numbered_suffixes),
        ("short_param_names_rs", Severity::Warning, checks_rs::style::check_short_param_names),
    ];

    let mut violations = Vec::new();
    for &(name, severity, check_fn) in rust_checks {
        let check_violations = check_fn(&parsed, &config);
        stamp_and_collect(name, severity, check_violations, &mut violations);
    }
    violations
}

/// Run all applicable gleipnir checks on a Svelte file.
///
/// Three phases:
/// 1. Script — extract `<script>`, parse with tree-sitter-typescript, run TS checks
/// 2. Style — extract `<style>`, parse with tree-sitter-css, run CSS architecture checks
/// 3. Template — extract template (everything outside script/style), parse with tree-sitter-html
/// 4. Raw source — checks that operate on raw text with path scoping
///
/// Line numbers are offset to match the .svelte file.
pub fn run_checks_svelte(file_path: &str, source: &[u8]) -> Vec<Violation> {
    let source_str = std::str::from_utf8(source).unwrap_or("");
    let mut violations = Vec::new();

    // Phase 1: Script block (TypeScript checks)
    run_svelte_script_checks(file_path, source_str, &mut violations);

    // Phase 2: Style block (CSS architecture checks)
    run_svelte_style_checks(file_path, source_str, &mut violations);

    // Phase 3: Template (HTML architecture checks)
    run_svelte_template_checks(file_path, source_str, &mut violations);

    // Phase 4: Raw source checks (path-scoped, no parsing)
    run_svelte_raw_checks(file_path, source_str, &mut violations);

    violations
}

fn run_svelte_script_checks(file_path: &str, source_str: &str, violations: &mut Vec<Violation>) {
    let script = match parsing::extract_svelte_script(source_str) {
        Some(s) => s,
        None => return,
    };

    let script_bytes = script.content.as_bytes();
    let parsed = match parsing::build_parsed_source_typescript(file_path, script_bytes) {
        Ok(p) => p,
        Err(_) => return,
    };
    let config = CheckConfig::for_typescript(&STATISTICS);

    type CheckFn = fn(&structures::ParsedSource, &CheckConfig) -> Vec<Violation>;
    let ts_checks: &[(&str, Severity, CheckFn)] = &[
        ("no_console_log", Severity::Error, checks_ts::prohibited::check_no_console_log),
        ("no_ts_suppression", Severity::Error, checks_ts::suppression::check_no_ts_suppression),
        ("function_length_ts", Severity::Warning, checks_ts::style::check_function_length),
        ("param_count_ts", Severity::Warning, checks_ts::style::check_param_count),
        ("nesting_depth_ts", Severity::Warning, checks_ts::style::check_nesting_depth),
        ("no_underscore_prefix_ts", Severity::Warning, checks_ts::style::check_no_underscore_prefix),
        ("no_single_letter_names_ts", Severity::Warning, checks_ts::style::check_no_single_letter_names),
        ("no_numbered_suffixes_ts", Severity::Warning, checks_ts::style::check_no_numbered_suffixes),
        ("short_param_names_ts", Severity::Warning, checks_ts::style::check_short_param_names),
    ];

    for &(name, severity, check_fn) in ts_checks {
        let mut check_violations = check_fn(&parsed, &config);
        for viol in &mut check_violations {
            viol.line += script.line_offset;
        }
        stamp_and_collect(name, severity, check_violations, violations);
    }
}

fn run_svelte_style_checks(file_path: &str, source_str: &str, violations: &mut Vec<Violation>) {
    let style = match parsing::extract_svelte_style(source_str) {
        Some(s) => s,
        None => return,
    };

    let style_bytes = style.content.as_bytes();
    let parsed = match parsing::build_parsed_source_css(file_path, style_bytes) {
        Ok(p) => p,
        Err(_) => return,
    };

    let css_checks: &[(&str, Severity, fn(&str, &ParsedSource) -> Vec<Violation>)] = &[
        ("no_100vh_in_components", Severity::Error, checks_svelte::architecture::check_no_100vh_in_components),
        ("no_hardcoded_colors", Severity::Warning, checks_svelte::architecture::check_no_hardcoded_colors),
        ("no_margin_in_shared", Severity::Error, checks_svelte::architecture::check_no_margin_in_shared),
        ("no_fixed_in_shared", Severity::Error, checks_svelte::architecture::check_no_fixed_in_shared),
    ];

    for &(name, severity, check_fn) in css_checks {
        let mut check_violations = check_fn(file_path, &parsed);
        for viol in &mut check_violations {
            viol.line += style.line_offset;
        }
        stamp_and_collect(name, severity, check_violations, violations);
    }
}

fn run_svelte_template_checks(file_path: &str, source_str: &str, violations: &mut Vec<Violation>) {
    let template = parsing::extract_svelte_template(source_str);
    if template.content.trim().is_empty() {
        return;
    }

    let template_bytes = template.content.as_bytes();
    let parsed = match parsing::build_parsed_source_html(file_path, template_bytes) {
        Ok(p) => p,
        Err(_) => return,
    };

    let html_checks: &[(&str, Severity, fn(&str, &ParsedSource) -> Vec<Violation>)] = &[
        ("no_raw_html_elements", Severity::Warning, checks_svelte::architecture::check_no_raw_html_elements),
    ];

    for &(name, severity, check_fn) in html_checks {
        let mut check_violations = check_fn(file_path, &parsed);
        for viol in &mut check_violations {
            viol.line += template.line_offset;
        }
        stamp_and_collect(name, severity, check_violations, violations);
    }
}

fn run_svelte_raw_checks(file_path: &str, source_str: &str, violations: &mut Vec<Violation>) {
    let raw_checks: &[(&str, Severity, fn(&str, &str) -> Vec<Violation>)] = &[
        ("no_missing_shared_imports", Severity::Error, checks_svelte::architecture::check_no_missing_shared_imports),
    ];

    for &(name, severity, check_fn) in raw_checks {
        let check_violations = check_fn(file_path, source_str);
        stamp_and_collect(name, severity, check_violations, violations);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_load_successfully() {
        let msgs = messages("no_any_types");
        assert!(!msgs.detail.is_empty());
        assert!(!msgs.signal.is_empty());
        assert!(!msgs.direction.is_empty());
        assert!(!msgs.canary.is_empty());
    }

    #[test]
    fn missing_check_returns_default() {
        let msgs = messages("nonexistent_check");
        assert!(msgs.detail.is_empty());
    }

    #[test]
    fn no_direct_file_io_messages_present() {
        let msgs = messages("no_direct_file_io");
        assert!(!msgs.detail.is_empty());
        assert!(!msgs.signal.is_empty());
        assert!(!msgs.direction.is_empty());
        assert!(!msgs.canary.is_empty());
    }

    #[test]
    fn run_checks_v2_blocks_direct_file_io_in_impure_simple() {
        let source = b"import tomllib\n";
        let violations = run_checks_v2("/project/src/pkg/logic/impure/registry/simple.py", source);
        let hit = violations
            .iter()
            .find(|v| v.check_name == "no_direct_file_io")
            .expect("no_direct_file_io must fire on import tomllib in the impure zone");
        assert_eq!(hit.severity, Severity::Blocked);
        assert!(hit.message.contains("read_toml_validate_json"));
        assert!(!hit.direction.is_empty(), "messages must be stamped");
    }

    #[test]
    fn run_checks_v2_script_shebang_not_subject_to_direct_file_io() {
        let source = b"#!/usr/bin/env -S uv run --script\nimport tomllib\n";
        let violations = run_checks_v2("/project/scripts/gen.py", source);
        assert!(violations.iter().all(|v| v.check_name != "no_direct_file_io"));
    }

    #[test]
    fn run_checks_on_empty_file() {
        let violations = run_checks("/test/empty.py", b"");
        assert!(violations.is_empty());
    }

    #[test]
    fn run_checks_classifies_test_file() {
        let source = b"def test_foo():\n    assert True\n";
        let violations = run_checks("/project/tests/test_foo.py", source);
        // Test files have a minimal check set — should run without panic
        let _ = violations;
    }

    #[test]
    fn run_checks_v2_catches_underscore_prefix() {
        let source = b"def _hidden_helper(x: int) -> int:\n    return x + 1\n";
        let violations = run_checks_v2(
            "/project/src/pkg/logic/orchestrate/compile/orchestrate.py",
            source,
        );
        let names: Vec<&str> = violations.iter().map(|v| v.check_name.as_str()).collect();
        assert!(names.contains(&"no_underscore_prefix"));
    }

    // -- statistics loading --

    #[test]
    fn statistics_load_successfully() {
        let stats = &*STATISTICS;
        assert!(stats.defaults.max_function_lines.is_some());
        assert!(stats.defaults.max_function_params.is_some());
    }

    #[test]
    fn for_kind_pure_function_gets_25() {
        let config = CheckConfig::for_kind(FileKind::PureFunction, &STATISTICS);
        assert_eq!(config.max_function_lines, 25);
    }

    #[test]
    fn for_kind_outside_gets_50() {
        let config = CheckConfig::for_kind(FileKind::Outside, &STATISTICS);
        assert_eq!(config.max_function_lines, 50);
    }

    #[test]
    fn for_v2_l2_cc_bounds() {
        let config = CheckConfig::for_v2(Level::L2, Zone::Pure, &STATISTICS);
        assert_eq!(config.min_cc, 1);
        assert_eq!(config.max_cc, 3);
    }

    #[test]
    fn for_v2_l4_cc_bounds() {
        let config = CheckConfig::for_v2(Level::L4, Zone::Pure, &STATISTICS);
        assert_eq!(config.min_cc, 4);
        assert_eq!(config.max_cc, 8);
    }

    #[test]
    fn for_rust_gets_defaults() {
        let config = CheckConfig::for_rust(&STATISTICS);
        assert_eq!(config.max_function_lines, 50);
        assert_eq!(config.max_function_params, 5);
    }

    #[test]
    fn for_typescript_gets_defaults() {
        let config = CheckConfig::for_typescript(&STATISTICS);
        assert_eq!(config.max_function_lines, 50);
        assert_eq!(config.max_nesting_depth, 4);
    }
}
