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
mod numbered_names;
pub mod parsing;
pub mod structures;

use std::sync::LazyLock;

pub use structures::{
    CheckConfig, CheckMessages, FileKind, Level, ParsedSource, Severity, StatisticsToml,
    V2Classification, Violation, Zone,
};

/// One constant per entry of gleipnir_messages.toml, compiled by the build script.
/// A check is named through its constant, so a check with no entry does not compile.
pub mod messages {
    use crate::structures::CheckMessages;

    include!(concat!(env!("OUT_DIR"), "/gleipnir_messages.rs"));
}

static STATISTICS_TOML: &str = include_str!("../gleipnir_statistics.toml");

pub static STATISTICS: LazyLock<StatisticsToml> = LazyLock::new(|| {
    toml::from_str(STATISTICS_TOML).expect("gleipnir_statistics.toml parse error")
});

/// Stamp a single violation with its check's name, severity and message fields.
/// A violation made by `CheckMessages::violation` already carries its own check's.
fn stamp(viol: &mut Violation, check: &CheckMessages, severity: Severity) {
    viol.severity = severity;
    if viol.check_name.is_empty() {
        viol.check_name = check.name.to_string();
        viol.detail = check.detail.to_string();
        viol.signal = check.signal.to_string();
        viol.direction = check.direction.to_string();
        viol.canary = check.canary.to_string();
    }
}

/// Stamp violations with check metadata and collect into output vec.
fn stamp_and_collect(
    check: &CheckMessages,
    severity: Severity,
    mut check_violations: Vec<Violation>,
    output: &mut Vec<Violation>,
) {
    for viol in &mut check_violations {
        stamp(viol, check, severity);
    }
    output.extend(check_violations);
}

/// Run all applicable gleipnir checks on a Python source file.
///
/// Classifies the file, selects checks from the matrix, parses with
/// tree-sitter, runs checks, returns violations. A source that cannot be
/// parsed is an error saying why: it was not examined.
pub fn run_checks(
    file_path: &str,
    source: &[u8],
) -> Result<Vec<Violation>, String> {
    let first_line = source
        .split(|&b| b == b'\n')
        .next()
        .and_then(|l| std::str::from_utf8(l).ok())
        .unwrap_or("");

    let kind = classify::classify_file(file_path, first_line);
    let config = CheckConfig::for_kind(kind, &STATISTICS);
    let entries = matrix::checks_for_kind(kind);

    let parsed = parsing::build_parsed_source(file_path, source)?;

    let mut violations = Vec::new();
    for entry in &entries {
        let check_violations = (entry.check_fn)(&parsed, &config);
        stamp_and_collect(entry.messages, entry.severity, check_violations, &mut violations);
    }
    Ok(violations)
}

/// Run v2 zone architecture checks on a Python source file.
///
/// Uses the two-axis classification (level + zone) to select checks.
/// Scripts and tests fall back to v1 rules.
pub fn run_checks_v2(
    file_path: &str,
    source: &[u8],
) -> Result<Vec<Violation>, String> {
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
    if classify::is_test_filename(filename) {
        return run_checks(file_path, source);
    }

    let classification = classify::classify_file_v2(file_path);
    let config = CheckConfig::for_v2(classification.level, classification.zone, &STATISTICS);
    let entries = matrix::checks_for_v2(&classification);

    let parsed = parsing::build_parsed_source(file_path, source)?;

    let mut violations = Vec::new();
    for entry in &entries {
        let check_violations = (entry.check_fn)(&parsed, &config);
        stamp_and_collect(entry.messages, entry.severity, check_violations, &mut violations);
    }
    Ok(violations)
}

/// Run all applicable gleipnir checks on a Rust source file.
///
/// Parses with tree-sitter-rust, runs Rust-specific checks, returns violations.
/// Skips test modules and #[test] functions for panic-on-failure checks.
pub fn run_checks_rust(file_path: &str, source: &[u8]) -> Result<Vec<Violation>, String> {
    let parsed = parsing::build_parsed_source_rust(file_path, source)?;
    let config = CheckConfig::for_rust(&STATISTICS);

    type CheckFn = fn(&structures::ParsedSource, &CheckConfig) -> Vec<Violation>;
    let rust_checks: &[(&CheckMessages, Severity, CheckFn)] = &[
        // PROHIBITED
        (&messages::NO_UNWRAP, Severity::Error, checks_rs::prohibited::check_no_unwrap),
        (&messages::NO_PRINTLN, Severity::Error, checks_rs::prohibited::check_no_println),
        (&messages::NO_CLONE_SPAM, Severity::Warning, checks_rs::prohibited::check_no_clone_spam),
        (&messages::NO_STRING_ABUSE, Severity::Warning, checks_rs::prohibited::check_no_string_abuse),
        (&messages::NO_PUB_OVERUSE, Severity::Warning, checks_rs::prohibited::check_no_pub_overuse),
        // SUPPRESSION
        (&messages::NO_SUPPRESSION_COMMENTS_RS, Severity::Error, checks_rs::suppression::check_no_suppression_comments),
        // STYLE
        (&messages::FUNCTION_LENGTH_RS, Severity::Warning, checks_rs::style::check_function_length),
        (&messages::PARAM_COUNT_RS, Severity::Warning, checks_rs::style::check_param_count),
        (&messages::NESTING_DEPTH_RS, Severity::Warning, checks_rs::style::check_nesting_depth),
        (&messages::NO_UNDERSCORE_PREFIX_RS, Severity::Warning, checks_rs::style::check_no_underscore_prefix),
        (&messages::NO_SINGLE_LETTER_NAMES_RS, Severity::Warning, checks_rs::style::check_no_single_letter_names),
        (&messages::NO_NUMBERED_SUFFIXES_RS, Severity::Warning, checks_rs::style::check_no_numbered_suffixes),
        (&messages::SHORT_PARAM_NAMES_RS, Severity::Warning, checks_rs::style::check_short_param_names),
    ];

    let mut violations = Vec::new();
    for &(check, severity, check_fn) in rust_checks {
        let check_violations = check_fn(&parsed, &config);
        stamp_and_collect(check, severity, check_violations, &mut violations);
    }
    Ok(violations)
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
pub fn run_checks_svelte(file_path: &str, source: &[u8]) -> Result<Vec<Violation>, String> {
    let source_str = std::str::from_utf8(source).map_err(|utf8_err| format!("the file is not UTF-8: {utf8_err}"))?;
    let mut violations = Vec::new();

    // Phase 1: Script block (TypeScript checks)
    run_svelte_script_checks(file_path, source_str, &mut violations)?;

    // Phase 2: Style block (CSS architecture checks)
    run_svelte_style_checks(file_path, source_str, &mut violations)?;

    // Phase 3: Template (HTML architecture checks)
    run_svelte_template_checks(file_path, source_str, &mut violations)?;

    // Phase 4: Raw source checks (path-scoped, no parsing)
    run_svelte_raw_checks(file_path, source_str, &mut violations);

    Ok(violations)
}

fn run_svelte_script_checks(file_path: &str, source_str: &str, violations: &mut Vec<Violation>) -> Result<(), String> {
    let script = match parsing::extract_svelte_script(source_str) {
        Some(s) => s,
        None => return Ok(()),
    };

    let script_bytes = script.content.as_bytes();
    let parsed = parsing::build_parsed_source_typescript(file_path, script_bytes)?;
    let config = CheckConfig::for_typescript(&STATISTICS);

    type CheckFn = fn(&structures::ParsedSource, &CheckConfig) -> Vec<Violation>;
    let ts_checks: &[(&CheckMessages, Severity, CheckFn)] = &[
        (&messages::NO_CONSOLE_LOG, Severity::Error, checks_ts::prohibited::check_no_console_log),
        (&messages::NO_TS_SUPPRESSION, Severity::Error, checks_ts::suppression::check_no_ts_suppression),
        (&messages::FUNCTION_LENGTH_TS, Severity::Warning, checks_ts::style::check_function_length),
        (&messages::PARAM_COUNT_TS, Severity::Warning, checks_ts::style::check_param_count),
        (&messages::NESTING_DEPTH_TS, Severity::Warning, checks_ts::style::check_nesting_depth),
        (&messages::NO_UNDERSCORE_PREFIX_TS, Severity::Warning, checks_ts::style::check_no_underscore_prefix),
        (&messages::NO_SINGLE_LETTER_NAMES_TS, Severity::Warning, checks_ts::style::check_no_single_letter_names),
        (&messages::NO_NUMBERED_SUFFIXES_TS, Severity::Warning, checks_ts::style::check_no_numbered_suffixes),
        (&messages::SHORT_PARAM_NAMES_TS, Severity::Warning, checks_ts::style::check_short_param_names),
    ];

    for &(check, severity, check_fn) in ts_checks {
        let mut check_violations = check_fn(&parsed, &config);
        for viol in &mut check_violations {
            viol.line += script.line_offset;
        }
        stamp_and_collect(check, severity, check_violations, violations);
    }
    Ok(())
}

fn run_svelte_style_checks(file_path: &str, source_str: &str, violations: &mut Vec<Violation>) -> Result<(), String> {
    let style = match parsing::extract_svelte_style(source_str) {
        Some(s) => s,
        None => return Ok(()),
    };

    let style_bytes = style.content.as_bytes();
    let parsed = parsing::build_parsed_source_css(file_path, style_bytes)?;

    let css_checks: &[(&CheckMessages, Severity, fn(&str, &ParsedSource) -> Vec<Violation>)] = &[
        (&messages::NO_100VH_IN_COMPONENTS, Severity::Error, checks_svelte::architecture::check_no_100vh_in_components),
        (&messages::NO_HARDCODED_COLORS, Severity::Warning, checks_svelte::architecture::check_no_hardcoded_colors),
        (&messages::NO_MARGIN_IN_SHARED, Severity::Error, checks_svelte::architecture::check_no_margin_in_shared),
        (&messages::NO_FIXED_IN_SHARED, Severity::Error, checks_svelte::architecture::check_no_fixed_in_shared),
    ];

    for &(check, severity, check_fn) in css_checks {
        let mut check_violations = check_fn(file_path, &parsed);
        for viol in &mut check_violations {
            viol.line += style.line_offset;
        }
        stamp_and_collect(check, severity, check_violations, violations);
    }
    Ok(())
}

fn run_svelte_template_checks(file_path: &str, source_str: &str, violations: &mut Vec<Violation>) -> Result<(), String> {
    let template = parsing::extract_svelte_template(source_str);
    if template.content.trim().is_empty() {
        return Ok(());
    }

    let template_bytes = template.content.as_bytes();
    let parsed = parsing::build_parsed_source_html(file_path, template_bytes)?;

    let html_checks: &[(&CheckMessages, Severity, fn(&str, &ParsedSource) -> Vec<Violation>)] = &[
        (&messages::NO_RAW_HTML_ELEMENTS, Severity::Warning, checks_svelte::architecture::check_no_raw_html_elements),
    ];

    for &(check, severity, check_fn) in html_checks {
        let mut check_violations = check_fn(file_path, &parsed);
        for viol in &mut check_violations {
            viol.line += template.line_offset;
        }
        stamp_and_collect(check, severity, check_violations, violations);
    }
    Ok(())
}

fn run_svelte_raw_checks(file_path: &str, source_str: &str, violations: &mut Vec<Violation>) {
    let raw_checks: &[(&CheckMessages, Severity, fn(&str, &str) -> Vec<Violation>)] = &[
        (&messages::NO_MISSING_SHARED_IMPORTS, Severity::Error, checks_svelte::architecture::check_no_missing_shared_imports),
    ];

    for &(check, severity, check_fn) in raw_checks {
        let check_violations = check_fn(file_path, source_str);
        stamp_and_collect(check, severity, check_violations, violations);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_violation_carries_its_check_s_four_message_fields() {
        let violations = run_checks_v2("/project/src/pkg/logic/impure/registry/simple.py", b"import tomllib\n")
            .expect("the source parses");
        let hit = violations
            .iter()
            .find(|v| v.check_name == "no_direct_file_io")
            .expect("no_direct_file_io must fire on import tomllib in the impure zone");
        let entry = messages::NO_DIRECT_FILE_IO;
        assert_eq!(
            (hit.detail.as_str(), hit.signal.as_str(), hit.direction.as_str(), hit.canary.as_str()),
            (entry.detail, entry.signal, entry.direction, entry.canary),
        );
    }

    #[test]
    fn a_check_that_names_itself_carries_its_own_fields_not_its_caller_s() {
        let source = b"def outer(value: int) -> int:\n    def inner() -> int:\n        return value\n    return inner()\n";
        let violations =
            run_checks_v2("/project/src/pkg/logic/pure/math/simple.py", source).expect("the source parses");
        let hit = violations
            .iter()
            .find(|v| v.check_name == "no_closures")
            .expect("no_closures must fire on a closure in the pure zone");
        assert_eq!(hit.direction, messages::NO_CLOSURES.direction);
        assert_ne!(hit.direction, messages::NO_NESTED_FUNCTIONS.direction);
    }

    #[test]
    fn run_checks_v2_blocks_direct_file_io_in_impure_simple() {
        let source = b"import tomllib\n";
        let violations =
            run_checks_v2("/project/src/pkg/logic/impure/registry/simple.py", source).expect("the source parses");
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
        let violations = run_checks_v2("/project/scripts/gen.py", source).expect("the source parses");
        assert!(violations.iter().all(|v| v.check_name != "no_direct_file_io"));
    }

    #[test]
    fn run_checks_on_empty_file() {
        let violations = run_checks("/test/empty.py", b"").expect("the source parses");
        assert!(violations.is_empty());
    }

    #[test]
    fn run_checks_classifies_test_file() {
        let source = b"def test_foo():\n    assert True\n";
        // Test files have a minimal check set — should run without panic
        run_checks("/project/tests/test_foo.py", source).expect("the source parses");
    }

    #[test]
    fn run_checks_v2_catches_underscore_prefix() {
        let source = b"def _hidden_helper(x: int) -> int:\n    return x + 1\n";
        let violations = run_checks_v2(
            "/project/src/pkg/logic/orchestrate/compile/orchestrate.py",
            source,
        )
        .expect("the source parses");
        let names: Vec<&str> = violations.iter().map(|v| v.check_name.as_str()).collect();
        assert!(names.contains(&"no_underscore_prefix"));
    }

    #[test]
    fn no_message_field_reveals_a_complexity_band() {
        let entries: toml::Table =
            toml::from_str(include_str!("../gleipnir_messages.toml")).expect("the messages parse");
        assert!(!entries.is_empty(), "the messages file has entries");
        let mut offenders = Vec::new();
        for (check_name, entry) in &entries {
            for field in ["detail", "signal", "direction"] {
                let text = entry.get(field).and_then(|value| value.as_str()).unwrap_or("");
                if text.contains("CC=") || text.contains("L1=") {
                    offenders.push(format!("{check_name}.{field}"));
                }
            }
        }
        assert!(offenders.is_empty(), "fields that reveal a band: {offenders:?}");
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
    fn for_v2_transform_overrides_cc_and_nesting_and_inherits_lines() {
        // (level, [v2.transform.<level>] max_cc, max_nesting_depth) as written in the statistics.
        let overridden = [(Level::L1, 1, 1), (Level::L2, 2, 2), (Level::L4, 4, 3)];
        for (level, max_cc, max_nesting_depth) in overridden {
            let base = CheckConfig::for_v2(level, Zone::Pure, &STATISTICS);
            let transform = CheckConfig::for_v2(level, Zone::Transform, &STATISTICS);
            assert_eq!(transform.max_cc, max_cc, "{level:?} max_cc");
            assert_eq!(transform.max_nesting_depth, max_nesting_depth, "{level:?} max_nesting_depth");
            assert_eq!(transform.max_function_lines, base.max_function_lines, "{level:?} lines inherit");
            assert_eq!(transform.min_cc, base.min_cc, "{level:?} min_cc inherits");
        }
    }

    #[test]
    fn for_v2_pure_is_untouched_by_transform_overrides() {
        // (level, [v2.<level>] max_cc, max_nesting_depth) as written in the statistics.
        let base_values = [(Level::L1, 1, 2), (Level::L2, 3, 3), (Level::L4, 8, 4)];
        for (level, max_cc, max_nesting_depth) in base_values {
            let config = CheckConfig::for_v2(level, Zone::Pure, &STATISTICS);
            assert_eq!(config.max_cc, max_cc, "{level:?} max_cc");
            assert_eq!(config.max_nesting_depth, max_nesting_depth, "{level:?} max_nesting_depth");
        }
    }

    #[test]
    fn for_v2_transform_without_override_equals_base() {
        let base = CheckConfig::for_v2(Level::L5, Zone::Pure, &STATISTICS);
        let transform = CheckConfig::for_v2(Level::L5, Zone::Transform, &STATISTICS);
        assert_eq!(transform.min_cc, base.min_cc);
        assert_eq!(transform.max_cc, base.max_cc);
        assert_eq!(transform.max_nesting_depth, base.max_nesting_depth);
        assert_eq!(transform.max_function_lines, base.max_function_lines);
        assert_eq!(transform.max_function_params, base.max_function_params);
    }

    #[test]
    fn for_v2_outside_has_no_cc_band_and_l8_has_one() {
        let outside = CheckConfig::for_v2(Level::Outside, Zone::Structure, &STATISTICS);
        assert_eq!((outside.min_cc, outside.max_cc), (0, usize::MAX));
        let entry_point = CheckConfig::for_v2(Level::L8, Zone::Orchestrate, &STATISTICS);
        assert_eq!((entry_point.min_cc, entry_point.max_cc), (1, 2));
    }

    #[test]
    fn run_checks_v2_routes_pytest_filename_to_v1_rules() {
        let source = b"import tomllib\n";
        let violations = run_checks_v2("/project/src/pkg/logic/impure/registry/test_simple.py", source)
            .expect("the source parses");
        assert!(violations.iter().all(|v| v.check_name != "no_direct_file_io"));
    }

    #[test]
    fn run_checks_v2_keeps_file_merely_containing_test_under_v2_rules() {
        let source = b"import tomllib\n";
        let violations = run_checks_v2("/project/src/pkg/logic/impure/latest/simple.py", source)
            .expect("the source parses");
        assert!(violations.iter().any(|v| v.check_name == "no_direct_file_io"));
        let violations = run_checks_v2("/project/src/pkg/logic/impure/registry/latest.py", source)
            .expect("the source parses");
        assert!(violations.iter().any(|v| v.check_name == "no_direct_file_io"));
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
