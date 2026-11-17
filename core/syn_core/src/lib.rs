//! Syn core: policy types and filter engine for quality gating.
//!
//! Pure computation library. No I/O — caller provides reports and config,
//! library returns filtered output with a policy decision.

use jaq_interpret::{Ctx, FilterT, ParseCtx, RcIter, Val};
use report_render_core::{CheckGroup, LocatedFault, collect_faults, group_issues, severity_rank};
use saga_core::{Issue, SanityReport};

// =============================================================================
// Filter engine (jaq-interpret)
// =============================================================================

pub type CompiledFilter = jaq_interpret::Filter;

pub fn compile_filter(expr: &str) -> Result<CompiledFilter, String> {
    let mut defs = ParseCtx::new(Vec::new());
    let (parsed, errs) = jaq_parse::parse(expr, jaq_parse::main());
    if !errs.is_empty() {
        return Err(format!("jq parse error in '{}': {:?}", expr, errs));
    }
    let filter = defs.compile(
        parsed.ok_or_else(|| format!("jq parse returned None for '{}'", expr))?,
    );
    if !defs.errs.is_empty() {
        return Err(format!(
            "jq compile error in '{}': {} errors",
            expr,
            defs.errs.len()
        ));
    }
    Ok(filter)
}

pub fn matches_filter(issue: &Issue, filter: &CompiledFilter) -> bool {
    let json = match serde_json::to_value(issue) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let inputs = RcIter::new(core::iter::empty());
    let val = Val::from(json);
    let results: Vec<_> = filter.run((Ctx::new([], &inputs), val)).collect();
    matches!(results.first(), Some(Ok(Val::Bool(true))))
}

// =============================================================================
// Config types
// =============================================================================

pub const DEFAULT_WARN_FILTER: &str = r#".tool == "gleipnir""#;
pub const DEFAULT_DENY_FILTER: &str = r#".severity == "blocked""#;

pub struct SynConfig {
    pub warn_filter: CompiledFilter,
    pub deny_filter: CompiledFilter,
}

// =============================================================================
// Policy types
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyMode {
    Report,
    Gate,
}

pub struct FilterOverrides {
    pub tool: Option<String>,
    pub level: Option<String>,
    pub custom_filter: Option<CompiledFilter>,
    pub narrow: Option<String>,
}

impl Default for FilterOverrides {
    fn default() -> Self {
        Self {
            tool: None,
            level: None,
            custom_filter: None,
            narrow: None,
        }
    }
}

pub struct FilteredOutput {
    pub warn_groups: Vec<CheckGroup>,
    /// Every fault the reports carry. No filter hides a fault.
    pub faults: Vec<LocatedFault>,
    pub deny_issues: usize,
    pub decision: &'static str,
}

// =============================================================================
// Three-tier filtering
// =============================================================================

fn is_visible(
    issue: &Issue,
    config: &SynConfig,
    has_cli_overrides: bool,
    overrides: &FilterOverrides,
) -> bool {
    if has_cli_overrides {
        if let Some(ref tool) = overrides.tool {
            if tool != "all" && issue.tool != *tool {
                return false;
            }
        }
        if let Some(ref level) = overrides.level {
            if severity_rank(&issue.severity) < severity_rank(level) {
                return false;
            }
        }
        if let Some(ref f) = overrides.custom_filter {
            if !matches_filter(issue, f) {
                return false;
            }
        }
        if let Some(ref code) = overrides.narrow {
            if issue.code != *code {
                return false;
            }
        }
        true
    } else {
        matches_filter(issue, &config.warn_filter)
    }
}

pub fn apply_filters(
    reports: Vec<SanityReport>,
    config: &SynConfig,
    mode: PolicyMode,
    overrides: &FilterOverrides,
) -> FilteredOutput {
    let faults = collect_faults(&reports);
    let mut visible_reports: Vec<SanityReport> = Vec::new();
    let mut deny_count: usize = 0;

    let has_cli_overrides = mode == PolicyMode::Report
        && (overrides.tool.is_some()
            || overrides.level.is_some()
            || overrides.custom_filter.is_some()
            || overrides.narrow.is_some());

    for mut report in reports {
        let visible_issues: Vec<_> = report
            .issues
            .into_iter()
            .filter(|issue| is_visible(issue, config, has_cli_overrides, overrides))
            .collect();

        for issue in &visible_issues {
            if matches_filter(issue, &config.deny_filter) {
                deny_count += 1;
            }
        }

        if !visible_issues.is_empty() {
            report.issues = visible_issues;
            visible_reports.push(report);
        }
    }

    let warn_groups = group_issues(&visible_reports);
    let decision = decision_of(mode, deny_count, !warn_groups.is_empty(), !faults.is_empty());

    FilteredOutput {
        warn_groups,
        faults,
        deny_issues: deny_count,
        decision,
    }
}

/// The one decision rule.
///
/// A gate answers whether a file was examined and is acceptable: it denies
/// for an issue the deny filter matches, and for any fault, because a fault
/// is a part of the examination that did not happen. Outside a gate, what
/// there is to show is a warning. Nothing to show is allow.
pub fn decision_of(mode: PolicyMode, deny_issues: usize, has_visible_issues: bool, has_faults: bool) -> &'static str {
    let gate_refuses = mode == PolicyMode::Gate && (deny_issues > 0 || has_faults);
    if gate_refuses {
        "deny"
    } else if has_visible_issues || has_faults {
        "warn"
    } else {
        "allow"
    }
}

/// The output with the faults found outside the reports joined to it — a
/// sidecar that is stale, unreadable or absent — and the decision taken again
/// with them counted.
pub fn with_faults(output: FilteredOutput, mode: PolicyMode, found_outside: Vec<LocatedFault>) -> FilteredOutput {
    let faults: Vec<LocatedFault> = found_outside.into_iter().chain(output.faults).collect();
    let decision = decision_of(mode, output.deny_issues, !output.warn_groups.is_empty(), !faults.is_empty());
    FilteredOutput { warn_groups: output.warn_groups, faults, deny_issues: output.deny_issues, decision }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_issue(tool: &str, code: &str, severity: &str, line: usize) -> Issue {
        Issue {
            tool: tool.into(),
            code: code.into(),
            severity: severity.into(),
            line,
            column: None,
            message: String::new(),
            category: String::new(),
            fixable: false,
            detail: String::new(),
            signal: String::new(),
            direction: String::new(),
            canary: String::new(),
        }
    }

    fn make_issue_fixable(tool: &str, code: &str, severity: &str, line: usize) -> Issue {
        Issue {
            fixable: true,
            ..make_issue(tool, code, severity, line)
        }
    }

    fn make_report(path: &str, issues: Vec<Issue>) -> SanityReport {
        SanityReport {
            file: path.into(),
            relative_path: path.into(),
            issues,
            ..Default::default()
        }
    }

    fn make_config(warn_expr: &str, deny_expr: &str) -> SynConfig {
        SynConfig {
            warn_filter: compile_filter(warn_expr).unwrap(),
            deny_filter: compile_filter(deny_expr).unwrap(),
        }
    }

    fn no_overrides() -> FilterOverrides {
        FilterOverrides::default()
    }

    // =========================================================================
    // compile_filter
    // =========================================================================

    #[test]
    fn compile_filter_valid_tool_eq() {
        let result = compile_filter(r#".tool == "gleipnir""#);
        assert!(result.is_ok(), "valid jq filter should compile: {:?}", result.err());
    }

    #[test]
    fn compile_filter_valid_severity_eq() {
        let result = compile_filter(r#".severity == "blocked""#);
        assert!(result.is_ok(), "severity equality filter should compile");
    }

    #[test]
    fn compile_filter_invalid_syntax() {
        let result = compile_filter(".[[[");
        assert!(result.is_err(), "malformed jq expression should fail to compile");
    }

    #[test]
    fn compile_filter_complex_and_expression() {
        let result = compile_filter(r#".tool == "ruff" and .severity == "error""#);
        assert!(result.is_ok(), "complex 'and' filter should compile: {:?}", result.err());
    }

    #[test]
    fn compile_filter_boolean_field() {
        let result = compile_filter(".fixable");
        assert!(result.is_ok(), "boolean field filter should compile: {:?}", result.err());
    }

    #[test]
    fn compile_filter_string_interpolation() {
        let result = compile_filter(r#".category == "style""#);
        assert!(result.is_ok(), "category string comparison should compile: {:?}", result.err());
    }

    #[test]
    fn compile_filter_empty_string() {
        let _ = compile_filter("");
    }

    // =========================================================================
    // matches_filter
    // =========================================================================

    #[test]
    fn matches_filter_tool_eq_matches() {
        let filter = compile_filter(r#".tool == "gleipnir""#).unwrap();
        let issue = make_issue("gleipnir", "G001", "error", 10);
        assert!(matches_filter(&issue, &filter));
    }

    #[test]
    fn matches_filter_tool_eq_no_match() {
        let filter = compile_filter(r#".tool == "gleipnir""#).unwrap();
        let issue = make_issue("ruff", "E501", "warning", 10);
        assert!(!matches_filter(&issue, &filter));
    }

    #[test]
    fn matches_filter_severity_eq_matches() {
        let filter = compile_filter(r#".severity == "error""#).unwrap();
        let issue = make_issue("ruff", "E501", "error", 10);
        assert!(matches_filter(&issue, &filter));
    }

    #[test]
    fn matches_filter_severity_eq_no_match() {
        let filter = compile_filter(r#".severity == "blocked""#).unwrap();
        let issue = make_issue("ruff", "E501", "warning", 10);
        assert!(!matches_filter(&issue, &filter));
    }

    #[test]
    fn matches_filter_fixable_true() {
        let filter = compile_filter(".fixable").unwrap();
        let issue = make_issue_fixable("ruff", "E501", "warning", 10);
        assert!(matches_filter(&issue, &filter));
    }

    #[test]
    fn matches_filter_fixable_false_no_match() {
        let filter = compile_filter(".fixable").unwrap();
        let issue = make_issue("ruff", "E501", "warning", 10);
        assert!(!matches_filter(&issue, &filter));
    }

    #[test]
    fn matches_filter_complex_and_both_match() {
        let filter = compile_filter(r#".tool == "ruff" and .severity == "error""#).unwrap();
        let issue = make_issue("ruff", "E501", "error", 10);
        assert!(matches_filter(&issue, &filter));
    }

    #[test]
    fn matches_filter_complex_and_one_fails() {
        let filter = compile_filter(r#".tool == "ruff" and .severity == "error""#).unwrap();
        let issue = make_issue("ruff", "E501", "warning", 10);
        assert!(!matches_filter(&issue, &filter));
    }

    #[test]
    fn matches_filter_or_expression() {
        let filter = compile_filter(r#".tool == "ruff" or .tool == "gleipnir""#).unwrap();
        assert!(matches_filter(&make_issue("ruff", "E501", "warning", 1), &filter));
        assert!(matches_filter(&make_issue("gleipnir", "G001", "error", 1), &filter));
        assert!(!matches_filter(&make_issue("basedpyright", "BP001", "info", 1), &filter));
    }

    #[test]
    fn matches_filter_line_number() {
        let filter = compile_filter(".line > 50").unwrap();
        assert!(matches_filter(&make_issue("ruff", "E501", "warning", 100), &filter));
        assert!(!matches_filter(&make_issue("ruff", "E501", "warning", 10), &filter));
    }

    // =========================================================================
    // apply_filters
    // =========================================================================

    #[test]
    fn warn_filter_selects_matching_tool() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let reports = vec![make_report("src/a.py", vec![
            make_issue("gleipnir", "G001", "error", 10),
            make_issue("ruff", "E501", "warning", 20),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Report, &no_overrides());
        let total = report_render_core::total_issues(&result.warn_groups);
        assert_eq!(total, 1);
        assert_eq!(result.warn_groups[0].tool, "gleipnir");
    }

    #[test]
    fn warn_filter_excludes_nonmatching() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let reports = vec![make_report("src/a.py", vec![
            make_issue("ruff", "E501", "warning", 10),
            make_issue("ruff", "F401", "error", 20),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Report, &no_overrides());
        assert!(result.warn_groups.is_empty());
        assert_eq!(result.decision, "allow");
    }

    #[test]
    fn deny_filter_counts_blocked() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let reports = vec![make_report("src/a.py", vec![
            make_issue("gleipnir", "G001", "blocked", 10),
            make_issue("gleipnir", "G002", "error", 20),
            make_issue("gleipnir", "G003", "blocked", 30),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Report, &no_overrides());
        assert_eq!(result.deny_issues, 2);
    }

    #[test]
    fn deny_filter_zero_when_no_match() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let reports = vec![make_report("src/a.py", vec![
            make_issue("gleipnir", "G001", "error", 10),
            make_issue("gleipnir", "G002", "warning", 20),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Report, &no_overrides());
        assert_eq!(result.deny_issues, 0);
    }

    #[test]
    fn gate_mode_deny_issues_returns_deny() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let reports = vec![make_report("src/a.py", vec![
            make_issue("gleipnir", "G001", "blocked", 10),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Gate, &no_overrides());
        assert_eq!(result.decision, "deny");
        assert_eq!(result.deny_issues, 1);
    }

    #[test]
    fn gate_mode_no_deny_with_issues_returns_warn() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let reports = vec![make_report("src/a.py", vec![
            make_issue("gleipnir", "G001", "error", 10),
            make_issue("gleipnir", "G002", "warning", 20),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Gate, &no_overrides());
        assert_eq!(result.decision, "warn");
        assert_eq!(result.deny_issues, 0);
    }

    #[test]
    fn report_mode_never_returns_deny() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let reports = vec![make_report("src/a.py", vec![
            make_issue("gleipnir", "G001", "blocked", 10),
            make_issue("gleipnir", "G002", "blocked", 20),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Report, &no_overrides());
        assert_ne!(result.decision, "deny");
        assert_eq!(result.decision, "warn");
        assert_eq!(result.deny_issues, 2);
    }

    #[test]
    fn cli_tool_override_replaces_warn_filter() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let overrides = FilterOverrides { tool: Some("ruff".into()), ..Default::default() };
        let reports = vec![make_report("src/a.py", vec![
            make_issue("gleipnir", "G001", "error", 10),
            make_issue("ruff", "E501", "warning", 20),
            make_issue("ruff", "F401", "error", 30),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Report, &overrides);
        let total = report_render_core::total_issues(&result.warn_groups);
        assert_eq!(total, 2);
        for group in &result.warn_groups {
            assert_eq!(group.tool, "ruff");
        }
    }

    #[test]
    fn cli_tool_all_shows_everything() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let overrides = FilterOverrides { tool: Some("all".into()), ..Default::default() };
        let reports = vec![make_report("src/a.py", vec![
            make_issue("gleipnir", "G001", "error", 10),
            make_issue("ruff", "E501", "warning", 20),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Report, &overrides);
        assert_eq!(report_render_core::total_issues(&result.warn_groups), 2);
    }

    #[test]
    fn cli_level_filter_error_hides_lower() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let overrides = FilterOverrides {
            tool: Some("all".into()),
            level: Some("error".into()),
            ..Default::default()
        };
        let reports = vec![make_report("src/a.py", vec![
            make_issue("ruff", "E501", "warning", 10),
            make_issue("ruff", "F401", "error", 20),
            make_issue("gleipnir", "G001", "blocked", 30),
            make_issue("gleipnir", "G002", "info", 40),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Report, &overrides);
        assert_eq!(report_render_core::total_issues(&result.warn_groups), 2);
    }

    #[test]
    fn cli_level_filter_blocked_most_restrictive() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let overrides = FilterOverrides {
            tool: Some("all".into()),
            level: Some("blocked".into()),
            ..Default::default()
        };
        let reports = vec![make_report("src/a.py", vec![
            make_issue("gleipnir", "G001", "error", 10),
            make_issue("gleipnir", "G002", "blocked", 20),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Report, &overrides);
        assert_eq!(report_render_core::total_issues(&result.warn_groups), 1);
    }

    #[test]
    fn cli_custom_filter() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let overrides = FilterOverrides {
            custom_filter: Some(compile_filter(r#".code == "E501""#).unwrap()),
            ..Default::default()
        };
        let reports = vec![make_report("src/a.py", vec![
            make_issue("ruff", "E501", "warning", 10),
            make_issue("ruff", "F401", "error", 20),
            make_issue("gleipnir", "G001", "error", 30),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Report, &overrides);
        assert_eq!(report_render_core::total_issues(&result.warn_groups), 1);
        assert_eq!(result.warn_groups[0].code, "E501");
    }

    #[test]
    fn empty_reports_returns_allow() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let result = apply_filters(vec![], &config, PolicyMode::Report, &no_overrides());
        assert_eq!(result.decision, "allow");
        assert!(result.warn_groups.is_empty());
        assert_eq!(result.deny_issues, 0);
    }

    #[test]
    fn gate_empty_reports_returns_allow() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let result = apply_filters(vec![], &config, PolicyMode::Gate, &no_overrides());
        assert_eq!(result.decision, "allow");
    }

    // ── The decision rule ────────────────────────────────────────

    #[test]
    fn a_gate_denies_what_it_could_not_judge() {
        assert_eq!(decision_of(PolicyMode::Gate, 0, false, true), "deny");
        assert_eq!(decision_of(PolicyMode::Gate, 0, true, true), "deny");
    }

    #[test]
    fn a_gate_passes_only_what_was_examined_and_is_acceptable() {
        assert_eq!(decision_of(PolicyMode::Gate, 0, false, false), "allow");
        assert_eq!(decision_of(PolicyMode::Gate, 0, true, false), "warn");
        assert_eq!(decision_of(PolicyMode::Gate, 1, true, false), "deny");
    }

    #[test]
    fn a_report_never_denies() {
        assert_eq!(decision_of(PolicyMode::Report, 3, true, true), "warn");
        assert_eq!(decision_of(PolicyMode::Report, 0, false, true), "warn");
        assert_eq!(decision_of(PolicyMode::Report, 0, false, false), "allow");
    }

    fn stale_sidecar_fault() -> LocatedFault {
        LocatedFault {
            file: "/project/.simple.py.qa".to_string(),
            tool: "syn".to_string(),
            mode: "sidecar_stale".to_string(),
            class: "validation_error".to_string(),
            message: "The sidecar is stale.".to_string(),
        }
    }

    #[test]
    fn a_fault_found_outside_the_reports_makes_a_clean_gate_deny() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let clean = apply_filters(vec![], &config, PolicyMode::Gate, &no_overrides());
        assert_eq!(clean.decision, "allow");
        let judged = with_faults(clean, PolicyMode::Gate, vec![stale_sidecar_fault()]);
        assert_eq!(judged.decision, "deny");
        assert_eq!(judged.faults.len(), 1);
    }

    #[test]
    fn a_fault_found_outside_the_reports_makes_a_clean_report_warn() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let clean = apply_filters(vec![], &config, PolicyMode::Report, &no_overrides());
        assert_eq!(with_faults(clean, PolicyMode::Report, vec![stale_sidecar_fault()]).decision, "warn");
    }

    #[test]
    fn no_fault_found_outside_leaves_the_decision_as_it_was() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let clean = apply_filters(vec![], &config, PolicyMode::Gate, &no_overrides());
        assert_eq!(with_faults(clean, PolicyMode::Gate, Vec::new()).decision, "allow");
    }

    #[test]
    fn no_matching_issues_returns_allow() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let reports = vec![make_report("src/a.py", vec![
            make_issue("ruff", "E501", "warning", 10),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Gate, &no_overrides());
        assert_eq!(result.decision, "allow");
    }

    #[test]
    fn multiple_reports_aggregates() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("gleipnir", "G001", "blocked", 10),
                make_issue("ruff", "E501", "warning", 20),
            ]),
            make_report("src/b.py", vec![
                make_issue("gleipnir", "G001", "error", 15),
                make_issue("gleipnir", "G002", "blocked", 25),
            ]),
        ];
        let result = apply_filters(reports, &config, PolicyMode::Gate, &no_overrides());
        assert_eq!(report_render_core::total_issues(&result.warn_groups), 3);
        assert_eq!(result.deny_issues, 2);
        assert_eq!(result.decision, "deny");
    }

    #[test]
    fn deny_only_counts_visible() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let reports = vec![make_report("src/a.py", vec![
            make_issue("ruff", "E501", "blocked", 10),
            make_issue("gleipnir", "G001", "warning", 20),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Gate, &no_overrides());
        assert_eq!(result.deny_issues, 0);
        assert_eq!(result.decision, "warn");
    }

    #[test]
    fn gate_mode_ignores_cli_overrides() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let overrides = FilterOverrides { tool: Some("ruff".into()), ..Default::default() };
        let reports = vec![make_report("src/a.py", vec![
            make_issue("gleipnir", "G001", "error", 10),
            make_issue("ruff", "E501", "warning", 20),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Gate, &overrides);
        assert_eq!(report_render_core::total_issues(&result.warn_groups), 1);
        assert_eq!(result.warn_groups[0].tool, "gleipnir");
    }

    #[test]
    fn passall_warn_shows_everything() {
        let config = make_config(".line > 0", r#".severity == "blocked""#);
        let reports = vec![make_report("src/a.py", vec![
            make_issue("gleipnir", "G001", "error", 10),
            make_issue("ruff", "E501", "warning", 20),
            make_issue("basedpyright", "BP001", "info", 30),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Report, &no_overrides());
        assert_eq!(report_render_core::total_issues(&result.warn_groups), 3);
    }

    #[test]
    fn passall_deny_counts_all_visible() {
        let config = make_config(".line > 0", ".line > 0");
        let reports = vec![make_report("src/a.py", vec![
            make_issue("ruff", "E501", "warning", 10),
            make_issue("gleipnir", "G001", "error", 20),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Gate, &no_overrides());
        assert_eq!(result.deny_issues, 2);
        assert_eq!(result.decision, "deny");
    }

    #[test]
    fn deny_passnothing_never_denies() {
        let config = make_config(".line > 0", r#".tool == "NONEXISTENT_TOOL_xyz""#);
        let reports = vec![make_report("src/a.py", vec![
            make_issue("gleipnir", "G001", "blocked", 10),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Gate, &no_overrides());
        assert_eq!(result.deny_issues, 0);
        assert_eq!(result.decision, "warn");
    }

    #[test]
    fn cli_tool_and_level_combined() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let overrides = FilterOverrides {
            tool: Some("ruff".into()),
            level: Some("error".into()),
            ..Default::default()
        };
        let reports = vec![make_report("src/a.py", vec![
            make_issue("ruff", "E501", "warning", 10),
            make_issue("ruff", "F401", "error", 20),
            make_issue("gleipnir", "G001", "blocked", 30),
        ])];
        let result = apply_filters(reports, &config, PolicyMode::Report, &overrides);
        assert_eq!(report_render_core::total_issues(&result.warn_groups), 1);
    }

    // =========================================================================
    // faults
    // =========================================================================

    #[test]
    fn a_fault_is_reported_whatever_the_filters_hide() {
        let config = make_config(r#".tool == "no-such-tool""#, r#".tool == "no-such-tool""#);
        let mut report = make_report("a.py", vec![make_issue("ruff", "E501", "warning", 1)]);
        report.faults.push(saga_core::Fault {
            tool: "basedpyright".into(),
            mode: "instrument_not_runnable".into(),
            class: "instrument_error".into(),
            message: "why".into(),
        });
        let result = apply_filters(vec![report], &config, PolicyMode::Report, &no_overrides());
        assert!(result.warn_groups.is_empty());
        assert_eq!(result.faults.len(), 1);
        assert_eq!(result.faults[0].file, "a.py");
        assert_eq!(result.decision, "warn");
    }
}
