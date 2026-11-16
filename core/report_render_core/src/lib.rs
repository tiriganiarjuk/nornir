//! Report rendering and grouping for QA sanity reports.
//!
//! Provides pure functions to group, format, and serialize `saga_core::SanityReport`
//! issues into human-readable (colored terminal, TOON) and machine-readable (JSON)
//! output.
//!
//! All public API is pure — no I/O, no side effects.

use std::collections::HashMap;

use saga_core::SanityReport;

// =============================================================================
// Grouped structures
// =============================================================================

#[derive(Debug, Clone)]
pub struct LocatedIssue {
    pub file: String,
    pub line: usize,
    pub message: String,
}

#[derive(Debug)]
pub struct CheckGroup {
    pub tool: String,
    pub code: String,
    pub severity: String,
    pub detail: String,
    pub signal: String,
    pub direction: String,
    pub canary: String,
    pub representative_message: String,
    pub issues: Vec<LocatedIssue>,
    pub file_count: usize,
}

/// A fault, with the file whose examination it interrupted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedFault {
    pub file: String,
    pub tool: String,
    pub mode: String,
    pub class: String,
    pub message: String,
}

/// Every fault the reports carry, each with its report's file.
pub fn collect_faults(reports: &[SanityReport]) -> Vec<LocatedFault> {
    reports
        .iter()
        .flat_map(|report| {
            report.faults.iter().map(|fault| LocatedFault {
                file: report.relative_path.clone(),
                tool: fault.tool.clone(),
                mode: fault.mode.clone(),
                class: fault.class.clone(),
                message: fault.message.clone(),
            })
        })
        .collect()
}

// =============================================================================
// Grouping
// =============================================================================

pub fn group_issues(reports: &[SanityReport]) -> Vec<CheckGroup> {
    // Index by borrowed (tool, code) → position in groups vec.
    // Avoids cloning key strings on every iteration.
    let mut index: HashMap<(&str, &str), usize> = HashMap::new();
    let mut groups: Vec<(saga_core::Issue, Vec<LocatedIssue>)> = Vec::new();

    for report in reports {
        for issue in &report.issues {
            let idx = match index.get(&(issue.tool.as_str(), issue.code.as_str())) {
                Some(&idx) => idx,
                None => {
                    let idx = groups.len();
                    index.insert((&issue.tool, &issue.code), idx);
                    groups.push((issue.clone(), Vec::new()));
                    idx
                }
            };
            groups[idx].1.push(LocatedIssue {
                file: report.relative_path.clone(),
                line: issue.line,
                message: issue.message.clone(),
            });
        }
    }

    let mut result: Vec<CheckGroup> = groups
        .into_iter()
        .map(|(rep, issues)| {
            let file_count = {
                let mut files: Vec<&str> = issues.iter().map(|li| li.file.as_str()).collect();
                files.sort();
                files.dedup();
                files.len()
            };
            let representative_message = issues.first()
                .map(|li| li.message.clone())
                .unwrap_or_default();
            CheckGroup {
                tool: rep.tool,
                code: rep.code,
                severity: rep.severity,
                detail: rep.detail,
                signal: rep.signal,
                direction: rep.direction,
                canary: rep.canary,
                representative_message,
                issues,
                file_count,
            }
        })
        .collect();

    // BTreeMap gave sorted output; maintain that contract.
    result.sort_by(|a, b| (&a.tool, &a.code).cmp(&(&b.tool, &b.code)));
    result
}

pub fn total_issues(groups: &[CheckGroup]) -> usize {
    groups.iter().map(|g| g.issues.len()).sum()
}

// =============================================================================
// Severity ordering
// =============================================================================

pub fn severity_rank(severity: &str) -> u8 {
    match severity {
        "info" => 0,
        "warning" => 1,
        "error" => 2,
        "blocked" => 3,
        _ => 0,
    }
}

// =============================================================================
// Output mode
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputMode {
    Toon,
    Colored,
    Json,
}

// =============================================================================
// Formatting primitives
// =============================================================================

const MAX_WIDTH: usize = 79;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const WHITE: &str = "\x1b[37m";
const BG_RED: &str = "\x1b[41m";

pub fn wrap_adaptive(text: &str, first_width: usize, rest_width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut width = first_width;
    for word in text.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
        } else if current.len() + 1 + word.len() > width {
            lines.push(current);
            current = word.to_string();
            width = rest_width;
        } else {
            current.push(' ');
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub fn collapse_line_numbers(lines: &[usize]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let mut count = 1;
        while i + count < lines.len() && lines[i + count] == line {
            count += 1;
        }
        if count == 1 {
            parts.push(line.to_string());
        } else {
            parts.push(format!("{}/{}", line, count));
        }
        i += count;
    }
    parts.join(",")
}

fn is_opaque_code(code: &str) -> bool {
    code.len() <= 8 && code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

fn severity_color(severity: &str) -> &'static str {
    match severity {
        "error" | "blocked" => "\x1b[1;31m",
        "warning" => "\x1b[1;33m",
        _ => "\x1b[1;37m",
    }
}

// =============================================================================
// Serialization
// =============================================================================

/// Who a rendered report is for. The canary is the operator's: it is left out of
/// what a worker session is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reader {
    Worker,
    Operator,
}

/// The report as JSON for the operator: the groups with each check's detail and
/// canary, and the faults. This is also the quality payload of a Hlidskjalf
/// datagram (yggdrasil `schemas/payloads/payload.quality.schema.json`).
pub fn report_to_json(groups: &[CheckGroup], faults: &[LocatedFault]) -> serde_json::Value {
    report_for(groups, faults, Reader::Operator)
}

/// The report as JSON for one reader. A worker's report carries no canary, and
/// names its faults only when it has any.
fn report_for(groups: &[CheckGroup], faults: &[LocatedFault], reader: Reader) -> serde_json::Value {
    let mut report = groups_to_json(groups);
    if let Some(json_groups) = report["groups"].as_array_mut() {
        for (json_group, group) in json_groups.iter_mut().zip(groups) {
            if let Some(fields) = json_group.as_object_mut() {
                *fields = reader_fields(std::mem::take(fields), &group.detail, reader);
            }
        }
    }
    if reader == Reader::Operator || !faults.is_empty() {
        let json_faults: Vec<serde_json::Value> = faults
            .iter()
            .map(|fault| {
                serde_json::json!({
                    "file": fault.file,
                    "tool": fault.tool,
                    "mode": fault.mode,
                    "class": fault.class,
                    "message": fault.message,
                })
            })
            .collect();
        report["faults"] = serde_json::Value::Array(json_faults);
    }
    report
}

/// One group's fields in reading order for a reader: the detail placed before the
/// signal, and the canary left out of a worker's.
fn reader_fields(
    payload_fields: serde_json::Map<String, serde_json::Value>,
    detail: &str,
    reader: Reader,
) -> serde_json::Map<String, serde_json::Value> {
    let mut fields = serde_json::Map::new();
    for (name, value) in payload_fields {
        if name == "canary" && reader == Reader::Worker {
            continue;
        }
        if name == "signal" {
            fields.insert("detail".to_string(), serde_json::Value::String(detail.to_string()));
        }
        fields.insert(name, value);
    }
    fields
}

/// The groups with the fields every reader is shown, before the reader's own are placed.
fn groups_to_json(groups: &[CheckGroup]) -> serde_json::Value {
    let json_groups: Vec<serde_json::Value> = groups
        .iter()
        .map(|group| {
            let mut file_lines: Vec<(&str, Vec<usize>)> = Vec::new();
            for li in &group.issues {
                if let Some(last) = file_lines.last_mut() {
                    if last.0 == li.file {
                        last.1.push(li.line);
                        continue;
                    }
                }
                file_lines.push((&li.file, vec![li.line]));
            }
            let locations: Vec<serde_json::Value> = file_lines.iter().map(|(file, lines)| {
                let path = if file.starts_with('/') || file.starts_with('@') {
                    file.to_string()
                } else {
                    format!("@{file}")
                };
                if lines.len() == 1 {
                    serde_json::json!({ "file": path, "line": lines[0] })
                } else {
                    serde_json::json!({ "file": path, "lines": lines })
                }
            }).collect();

            serde_json::json!({
                "tool": group.tool,
                "code": group.code,
                "message": group.representative_message,
                "severity": group.severity,
                "count": group.issues.len(),
                "file_count": group.file_count,
                "signal": group.signal,
                "direction": group.direction,
                "canary": group.canary,
                "locations": locations,
            })
        })
        .collect();

    serde_json::json!({
        "total": total_issues(groups),
        "check_types": groups.len(),
        "groups": json_groups,
    })
}

// =============================================================================
// Formatters
// =============================================================================

const VIOLATIONS_CLOSING: &str =
    "Who wrote this code? You did. Every violation above is yours to fix or explicitly defer with a reason.";
const FAULTS_CLOSING: &str = "A fault above means part of the examination did not happen: that file is not \
                              known to be clean. Report each fault to the user.";

fn format_toon(groups: &[CheckGroup], faults: &[LocatedFault]) -> String {
    if groups.is_empty() && faults.is_empty() {
        return "All checks passed.".to_string();
    }

    let json = report_for(groups, faults, Reader::Worker);
    let body = match format_core::serialize::to_toon(&json) {
        Ok(toon) => toon,
        Err(_) => {
            serde_json::to_string_pretty(&json).unwrap_or_default()
        }
    };
    let mut closing: Vec<&str> = Vec::new();
    if !groups.is_empty() {
        closing.push(VIOLATIONS_CLOSING);
    }
    if !faults.is_empty() {
        closing.push(FAULTS_CLOSING);
    }
    format!("{body}\n\n{}", closing.join("\n\n"))
}

fn format_colored(groups: &[CheckGroup], faults: &[LocatedFault]) -> String {
    if groups.is_empty() && faults.is_empty() {
        return format!(
            "\n{BOLD}{CYAN}{bar}{RESET}\n  All checks passed.\n{BOLD}{CYAN}{bar}{RESET}",
            bar = "━".repeat(MAX_WIDTH),
        );
    }

    let total = total_issues(groups);
    let mut sections: Vec<String> = Vec::new();

    let bar = "━".repeat(MAX_WIDTH);
    sections.push(format!("\n{BOLD}{RED}{bar}{RESET}"));
    sections.push(format!(
        "{BOLD}{WHITE}{BG_RED}  SYN  {RESET}  {BOLD}{WHITE}{total} violations{RESET} across {BOLD}{WHITE}{count} check types{RESET}, {BOLD}{WHITE}{fault_count} faults{RESET}",
        count = groups.len(),
        fault_count = faults.len(),
    ));
    sections.push(format!("{BOLD}{RED}{bar}{RESET}"));

    format_colored_faults(faults, &mut sections);
    for group in groups {
        format_colored_group(group, &mut sections);
    }

    sections.join("\n")
}

/// The faults block: each file that was not fully examined, the instrument, and why.
fn format_colored_faults(faults: &[LocatedFault], sections: &mut Vec<String>) {
    if faults.is_empty() {
        return;
    }
    let color = severity_color("blocked");
    sections.push(format!(
        "\n{color}┌─ FAULTS{RESET}  {BOLD}{WHITE}{count}{RESET} — not fully examined, so not known to be clean",
        count = faults.len(),
    ));
    let continuation = format!("{color}│{RESET}    ");
    for fault in faults {
        sections.push(format!("{color}│{RESET}"));
        sections.push(format!("{color}│{RESET}  {DIM}{}{RESET}", fault.file));
        let text = format!("{} ({}): {}", fault.tool, fault.mode, fault.message);
        for line in wrap_adaptive(&text, MAX_WIDTH - 5, MAX_WIDTH - 5) {
            sections.push(format!("{continuation}{line}"));
        }
    }
    sections.push(format!("{color}└{bar}{RESET}", bar = "─".repeat(MAX_WIDTH - 1)));
}

fn format_colored_group(group: &CheckGroup, sections: &mut Vec<String>) {
    let color = severity_color(&group.severity);
    let files_word = if group.file_count == 1 { "file" } else { "files" };
    let code_display = format_code_display(group);

    sections.push(format!(
        "\n{color}┌─ {code_display}{RESET}  {DIM}{severity}{RESET}  {BOLD}{WHITE}{count}{RESET} in {file_count} {files_word}",
        severity = group.severity,
        count = group.issues.len(), file_count = group.file_count,
    ));
    sections.push(format!("{color}│{RESET}"));

    let gutter = format!("{color}│{RESET}  ");
    let continuation = format!("{color}│{RESET}    ");
    let cont_width = MAX_WIDTH - 5;

    let layout = GutterLayout { gutter: &gutter, continuation: &continuation, cont_width };
    format_guidance_field(&group.detail, "Detail", CYAN, &layout, sections);
    format_guidance_field(&group.signal, "Signal", CYAN, &layout, sections);
    format_guidance_field(&group.direction, "Direction", CYAN, &layout, sections);
    format_guidance_field(&group.canary, "Canary", YELLOW, &layout, sections);

    sections.push(format!("{color}│{RESET}"));

    let file_lines = collect_file_lines(&group.issues);
    for (file, lines) in &file_lines {
        format_file_location(file, lines, color, sections);
    }
    sections.push(format!("{color}└{bar}{RESET}", bar = "─".repeat(MAX_WIDTH - 1)));
}

fn format_code_display(group: &CheckGroup) -> String {
    if is_opaque_code(&group.code) && !group.representative_message.is_empty() {
        let message = &group.representative_message;
        let short = message.find(". ").map(|i| &message[..i]).unwrap_or(message);
        let short = if short.len() > 60 {
            let truncated = &short[..short[..57].rfind(' ').unwrap_or(57)];
            format!("{}...", truncated)
        } else {
            short.to_string()
        };
        format!("{} — {}", group.code, short)
    } else {
        group.code.clone()
    }
}

struct GutterLayout<'a> {
    gutter: &'a str,
    continuation: &'a str,
    cont_width: usize,
}

fn format_guidance_field(
    text: &str, label: &str, label_color: &str,
    layout: &GutterLayout, sections: &mut Vec<String>,
) {
    if text.is_empty() {
        return;
    }
    let first_width = MAX_WIDTH - 4 - label.len() - 1;
    let lines = wrap_adaptive(text, first_width, layout.cont_width);
    sections.push(format!("{}{BOLD}{label_color}{label}:{RESET} {}", layout.gutter, lines[0]));
    for line in &lines[1..] {
        sections.push(format!("{}{line}", layout.continuation));
    }
}

fn collect_file_lines<'a>(issues: &'a [LocatedIssue]) -> Vec<(&'a str, Vec<usize>)> {
    let mut file_lines: Vec<(&str, Vec<usize>)> = Vec::new();
    for issue in issues {
        if let Some(last) = file_lines.last_mut() {
            if last.0 == issue.file {
                last.1.push(issue.line);
                continue;
            }
        }
        file_lines.push((&issue.file, vec![issue.line]));
    }
    file_lines
}

fn format_file_location(file: &str, lines: &[usize], color: &str, sections: &mut Vec<String>) {
    let line_list = format!("[{}]", collapse_line_numbers(lines));
    let avail = MAX_WIDTH - 3;

    if 3 + file.len() + 1 + line_list.len() <= MAX_WIDTH {
        let pad = avail - file.len() - line_list.len();
        sections.push(format!(
            "{color}│{RESET}  {DIM}{file}{}{line_list}{RESET}",
            " ".repeat(pad),
        ));
        return;
    }

    sections.push(format!("{color}│{RESET}  {DIM}{file}{RESET}"));

    if line_list.len() <= avail {
        let pad = avail - line_list.len();
        sections.push(format!(
            "{color}│{RESET}  {DIM}{}{line_list}{RESET}",
            " ".repeat(pad),
        ));
        return;
    }

    let collapsed = collapse_line_numbers(lines);
    let parts: Vec<&str> = collapsed.split(',').collect();
    let mut rows: Vec<String> = Vec::new();
    let mut current = String::from("[");
    for part in &parts {
        let entry = if current == "[" {
            part.to_string()
        } else {
            format!(",{}", part)
        };
        if current.len() + entry.len() + 1 > avail {
            current.push(',');
            rows.push(current);
            current = part.to_string();
        } else {
            current.push_str(&entry);
        }
    }
    current.push(']');
    rows.push(current);

    for row in &rows {
        let pad = if row.len() < avail { avail - row.len() } else { 0 };
        sections.push(format!(
            "{color}│{RESET}  {DIM}{}{row}{RESET}",
            " ".repeat(pad),
        ));
    }
}

fn format_json(groups: &[CheckGroup], faults: &[LocatedFault]) -> String {
    if groups.is_empty() && faults.is_empty() {
        return r#"{"total":0,"check_types":0,"groups":[],"faults":[]}"#.to_string();
    }
    serde_json::to_string_pretty(&report_to_json(groups, faults)).unwrap_or_default()
}

/// The report in one output mode. TOON is what a worker session is shown; colored
/// and JSON are the operator's.
pub fn format_output(groups: &[CheckGroup], faults: &[LocatedFault], output_mode: OutputMode) -> String {
    match output_mode {
        OutputMode::Toon => format_toon(groups, faults),
        OutputMode::Colored => format_colored(groups, faults),
        OutputMode::Json => format_json(groups, faults),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Test helpers
    // =========================================================================

    fn make_issue(
        tool: &str,
        code: &str,
        severity: &str,
        line: usize,
        message: &str,
    ) -> saga_core::Issue {
        saga_core::Issue {
            tool: tool.into(),
            code: code.into(),
            severity: severity.into(),
            line,
            column: None,
            message: message.into(),
            category: String::new(),
            fixable: false,
            detail: String::new(),
            signal: String::new(),
            direction: String::new(),
            canary: String::new(),
        }
    }

    fn make_report(
        relative_path: &str,
        issues: Vec<saga_core::Issue>,
    ) -> saga_core::SanityReport {
        saga_core::SanityReport {
            file: format!("/project/{}", relative_path),
            relative_path: relative_path.into(),
            issues,
            ..Default::default()
        }
    }

    // =========================================================================
    // severity_rank
    // =========================================================================

    #[test]
    fn severity_rank_info() {
        assert_eq!(severity_rank("info"), 0);
    }

    #[test]
    fn severity_rank_warning() {
        assert_eq!(severity_rank("warning"), 1);
    }

    #[test]
    fn severity_rank_error() {
        assert_eq!(severity_rank("error"), 2);
    }

    #[test]
    fn severity_rank_blocked() {
        assert_eq!(severity_rank("blocked"), 3);
    }

    #[test]
    fn severity_rank_unknown_returns_zero() {
        assert_eq!(severity_rank("critical"), 0);
        assert_eq!(severity_rank("fatal"), 0);
        assert_eq!(severity_rank("WARN"), 0);
    }

    #[test]
    fn severity_rank_empty_returns_zero() {
        assert_eq!(severity_rank(""), 0);
    }

    // =========================================================================
    // collapse_line_numbers
    // =========================================================================

    #[test]
    fn collapse_duplicates_with_mixed() {
        // Two 14s, then unique 21, 22
        assert_eq!(collapse_line_numbers(&[14, 14, 21, 22]), "14/2,21,22");
    }

    #[test]
    fn collapse_single_line() {
        assert_eq!(collapse_line_numbers(&[1]), "1");
    }

    #[test]
    fn collapse_triple_duplicate() {
        assert_eq!(collapse_line_numbers(&[5, 5, 5]), "5/3");
    }

    #[test]
    fn collapse_empty_input() {
        assert_eq!(collapse_line_numbers(&[]), "");
    }

    #[test]
    fn collapse_no_duplicates() {
        assert_eq!(collapse_line_numbers(&[1, 2, 3]), "1,2,3");
    }

    #[test]
    fn collapse_adjacent_groups() {
        // Two groups of duplicates next to each other
        assert_eq!(collapse_line_numbers(&[10, 10, 20, 20]), "10/2,20/2");
    }

    #[test]
    fn collapse_single_then_duplicate() {
        assert_eq!(collapse_line_numbers(&[1, 5, 5]), "1,5/2");
    }

    // =========================================================================
    // wrap_adaptive
    // =========================================================================

    #[test]
    fn wrap_short_text_fits_first_width() {
        let result = wrap_adaptive("hello world", 40, 30);
        assert_eq!(result, vec!["hello world"]);
    }

    #[test]
    fn wrap_text_exceeds_first_width() {
        // first_width = 10 → "hello" fits, "world" doesn't fit on same line
        let result = wrap_adaptive("hello world foo", 10, 20);
        // "hello" is first word → current = "hello" (5 chars)
        // "world": 5 + 1 + 5 = 11 > 10 → wrap, width becomes 20
        // "foo": 5 + 1 + 3 = 9 <= 20 → fits
        assert_eq!(result, vec!["hello", "world foo"]);
    }

    #[test]
    fn wrap_empty_string() {
        let result = wrap_adaptive("", 40, 30);
        assert_eq!(result, vec![""]);
    }

    #[test]
    fn wrap_single_long_word_not_broken() {
        let word = "supercalifragilisticexpialidocious";
        let result = wrap_adaptive(word, 10, 10);
        // Single word is never broken mid-word
        assert_eq!(result, vec![word]);
    }

    #[test]
    fn wrap_whitespace_only_input() {
        let result = wrap_adaptive("   ", 40, 30);
        // split_whitespace yields nothing, so we get vec![""]
        assert_eq!(result, vec![""]);
    }

    #[test]
    fn wrap_multiple_lines_continuation() {
        // Each word is 4 chars, first_width = 5 (fits one word),
        // rest_width = 5 (fits one word each)
        let result = wrap_adaptive("aaaa bbbb cccc", 5, 5);
        // "aaaa" → current (4), "bbbb": 4+1+4=9>5 → wrap, "cccc": 4+1+4=9>5 → wrap
        assert_eq!(result, vec!["aaaa", "bbbb", "cccc"]);
    }

    // =========================================================================
    // total_issues
    // =========================================================================

    #[test]
    fn total_issues_empty_groups() {
        assert_eq!(total_issues(&[]), 0);
    }

    #[test]
    fn total_issues_multiple_groups() {
        let groups = vec![
            CheckGroup {
                tool: "ruff".into(),
                code: "E501".into(),
                severity: "warning".into(),
                detail: String::new(),
                signal: String::new(),
                direction: String::new(),
                canary: String::new(),
                representative_message: "line too long".into(),
                issues: vec![
                    LocatedIssue {
                        file: "a.py".into(),
                        line: 1,
                        message: "line too long".into(),
                    },
                    LocatedIssue {
                        file: "b.py".into(),
                        line: 2,
                        message: "line too long".into(),
                    },
                ],
                file_count: 2,
            },
            CheckGroup {
                tool: "ruff".into(),
                code: "F401".into(),
                severity: "error".into(),
                detail: String::new(),
                signal: String::new(),
                direction: String::new(),
                canary: String::new(),
                representative_message: "unused import".into(),
                issues: vec![LocatedIssue {
                    file: "c.py".into(),
                    line: 5,
                    message: "unused import".into(),
                }],
                file_count: 1,
            },
        ];
        assert_eq!(total_issues(&groups), 3);
    }

    // =========================================================================
    // group_issues
    // =========================================================================

    #[test]
    fn group_issues_empty_reports() {
        let groups = group_issues(&[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn group_issues_same_tool_code_different_files() {
        let reports = vec![
            make_report("src/a.py", vec![make_issue("ruff", "E501", "warning", 10, "line too long")]),
            make_report("src/b.py", vec![make_issue("ruff", "E501", "warning", 20, "line too long")]),
        ];
        let groups = group_issues(&reports);
        assert_eq!(groups.len(), 1, "same tool+code should produce one group");
        assert_eq!(groups[0].tool, "ruff");
        assert_eq!(groups[0].code, "E501");
        assert_eq!(groups[0].file_count, 2);
        assert_eq!(groups[0].issues.len(), 2);
    }

    #[test]
    fn group_issues_different_tool_code() {
        let reports = vec![make_report(
            "src/a.py",
            vec![
                make_issue("ruff", "E501", "warning", 10, "line too long"),
                make_issue("ruff", "F401", "error", 5, "unused import"),
            ],
        )];
        let groups = group_issues(&reports);
        assert_eq!(groups.len(), 2, "different codes should produce separate groups");
    }

    #[test]
    fn group_issues_carries_severity_signal_direction_canary() {
        let mut issue = make_issue("gleipnir", "G001", "error", 1, "bad pattern");
        issue.signal = "Detected anti-pattern".into();
        issue.direction = "Use functional style".into();
        issue.canary = "canary-token-123".into();

        let reports = vec![make_report("src/x.py", vec![issue])];
        let groups = group_issues(&reports);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].severity, "error");
        assert_eq!(groups[0].signal, "Detected anti-pattern");
        assert_eq!(groups[0].direction, "Use functional style");
        assert_eq!(groups[0].canary, "canary-token-123");
    }

    #[test]
    fn group_issues_representative_message_from_first() {
        let reports = vec![
            make_report("src/a.py", vec![make_issue("ruff", "E501", "warning", 1, "first message")]),
            make_report("src/b.py", vec![make_issue("ruff", "E501", "warning", 2, "second message")]),
        ];
        let groups = group_issues(&reports);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].representative_message, "first message");
    }

    #[test]
    fn group_issues_same_file_multiple_lines() {
        let reports = vec![make_report(
            "src/a.py",
            vec![
                make_issue("ruff", "E501", "warning", 10, "line too long"),
                make_issue("ruff", "E501", "warning", 20, "line too long"),
            ],
        )];
        let groups = group_issues(&reports);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].file_count, 1);
        assert_eq!(groups[0].issues.len(), 2);
    }

    // =========================================================================
    // groups_to_json
    // =========================================================================

    #[test]
    fn groups_to_json_empty() {
        let json = groups_to_json(&[]);
        assert_eq!(json["total"], 0);
        assert_eq!(json["check_types"], 0);
        assert_eq!(json["groups"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn groups_to_json_structure() {
        let reports = vec![
            make_report("src/a.py", vec![make_issue("ruff", "E501", "warning", 10, "too long")]),
            make_report("src/b.py", vec![make_issue("ruff", "E501", "warning", 20, "too long")]),
        ];
        let groups = group_issues(&reports);
        let json = groups_to_json(&groups);

        assert_eq!(json["total"], 2);
        assert_eq!(json["check_types"], 1);

        let g = &json["groups"][0];
        assert_eq!(g["tool"], "ruff");
        assert_eq!(g["code"], "E501");
        assert_eq!(g["severity"], "warning");
        assert_eq!(g["count"], 2);
        assert_eq!(g["file_count"], 2);

        let locations = g["locations"].as_array().unwrap();
        assert_eq!(locations.len(), 2);
        // Each location is a different file with a single line
        assert_eq!(locations[0]["file"], "@src/a.py");
        assert_eq!(locations[0]["line"], 10);
        assert_eq!(locations[1]["file"], "@src/b.py");
        assert_eq!(locations[1]["line"], 20);
    }

    #[test]
    fn groups_to_json_same_file_collapses_lines() {
        let reports = vec![make_report(
            "src/a.py",
            vec![
                make_issue("ruff", "E501", "warning", 10, "too long"),
                make_issue("ruff", "E501", "warning", 25, "too long"),
            ],
        )];
        let groups = group_issues(&reports);
        let json = groups_to_json(&groups);

        let locations = json["groups"][0]["locations"].as_array().unwrap();
        assert_eq!(locations.len(), 1, "same file should collapse into one location");
        assert_eq!(locations[0]["file"], "@src/a.py");
        // Multiple lines → "lines" array (not "line" scalar)
        let lines = locations[0]["lines"].as_array().unwrap();
        assert_eq!(lines, &[10, 25]);
        assert!(locations[0]["line"].is_null(), "should use 'lines' not 'line' for multi-line");
    }

    #[test]
    fn groups_to_json_single_line_uses_line_not_lines() {
        let reports = vec![make_report(
            "src/a.py",
            vec![make_issue("ruff", "E501", "warning", 42, "too long")],
        )];
        let groups = group_issues(&reports);
        let json = groups_to_json(&groups);

        let loc = &json["groups"][0]["locations"][0];
        assert_eq!(loc["line"], 42);
        assert!(loc["lines"].is_null(), "single line should use 'line' not 'lines'");
    }

    #[test]
    fn groups_to_json_includes_signal_direction_canary() {
        let mut issue = make_issue("gleipnir", "G001", "error", 1, "bad");
        issue.signal = "sig".into();
        issue.direction = "dir".into();
        issue.canary = "can".into();

        let reports = vec![make_report("x.py", vec![issue])];
        let groups = group_issues(&reports);
        let json = groups_to_json(&groups);

        let g = &json["groups"][0];
        assert_eq!(g["signal"], "sig");
        assert_eq!(g["direction"], "dir");
        assert_eq!(g["canary"], "can");
    }

    // =========================================================================
    // format_output
    // =========================================================================

    #[test]
    fn format_output_json_empty() {
        let output = format_output(&[], &[], OutputMode::Json);
        assert!(output.contains("\"total\":0"), "empty JSON should contain total:0");
    }

    #[test]
    fn format_output_colored_empty() {
        let output = format_output(&[], &[], OutputMode::Colored);
        assert!(
            output.contains("All checks passed"),
            "empty colored output should say all checks passed"
        );
    }

    #[test]
    fn format_output_toon_empty() {
        let output = format_output(&[], &[], OutputMode::Toon);
        assert!(
            output.contains("All checks passed"),
            "empty toon output should say all checks passed"
        );
    }

    #[test]
    fn format_output_json_nonempty() {
        let reports = vec![make_report(
            "a.py",
            vec![make_issue("ruff", "E501", "warning", 1, "too long")],
        )];
        let groups = group_issues(&reports);
        let output = format_output(&groups, &[], OutputMode::Json);
        assert!(!output.is_empty());
        assert!(output.contains("\"total\""));
        assert!(output.contains("\"groups\""));
    }

    #[test]
    fn format_output_colored_nonempty() {
        let reports = vec![make_report(
            "a.py",
            vec![make_issue("ruff", "E501", "warning", 1, "too long")],
        )];
        let groups = group_issues(&reports);
        let output = format_output(&groups, &[], OutputMode::Colored);
        assert!(!output.is_empty());
        assert!(output.contains("violations"));
    }

    #[test]
    fn format_output_toon_nonempty() {
        let reports = vec![make_report(
            "a.py",
            vec![make_issue("ruff", "E501", "warning", 1, "too long")],
        )];
        let groups = group_issues(&reports);
        let output = format_output(&groups, &[], OutputMode::Toon);
        assert!(!output.is_empty());
    }

    // =========================================================================
    // detail, canary and faults — who is shown what
    // =========================================================================

    fn gleipnir_groups() -> Vec<CheckGroup> {
        let mut issue = make_issue("gleipnir", "no_print", "error", 3, "print call");
        issue.detail = "the-rule-text".into();
        issue.direction = "the-way-forward".into();
        issue.canary = "the-gaming-move".into();
        group_issues(&[make_report("a.py", vec![issue])])
    }

    fn ruff_fault() -> LocatedFault {
        LocatedFault {
            file: "a.py".into(),
            tool: "ruff".into(),
            mode: "instrument_not_runnable".into(),
            class: "instrument_error".into(),
            message: "ruff could not be run on 'a.py': No such file or directory.".into(),
        }
    }

    #[test]
    fn the_worker_s_report_carries_the_detail_and_no_canary() {
        let output = format_output(&gleipnir_groups(), &[], OutputMode::Toon);
        assert!(output.contains("the-rule-text"));
        assert!(output.contains("the-way-forward"));
        assert!(!output.contains("the-gaming-move"));
        assert!(!output.contains("canary"));
    }

    #[test]
    fn the_operator_s_reports_carry_the_detail_and_the_canary() {
        for mode in [OutputMode::Json, OutputMode::Colored] {
            let output = format_output(&gleipnir_groups(), &[], mode);
            assert!(output.contains("the-rule-text"));
            assert!(output.contains("the-gaming-move"));
        }
    }

    #[test]
    fn a_fault_alone_is_never_reported_as_all_checks_passed() {
        for mode in [OutputMode::Toon, OutputMode::Json, OutputMode::Colored] {
            let output = format_output(&[], &[ruff_fault()], mode);
            assert!(!output.contains("All checks passed"));
            assert!(output.contains("ruff could not be run on 'a.py'"));
        }
    }

    #[test]
    fn collect_faults_names_each_fault_s_file() {
        let mut report = make_report("src/a.py", Vec::new());
        report.faults.push(saga_core::Fault {
            tool: "ruff".into(),
            mode: "instrument_not_runnable".into(),
            class: "instrument_error".into(),
            message: "why".into(),
        });
        let faults = collect_faults(&[report]);
        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0].file, "src/a.py");
        assert_eq!(faults[0].tool, "ruff");
    }

    #[test]
    fn the_operator_s_report_always_names_its_faults_even_when_there_are_none() {
        let report = report_to_json(&gleipnir_groups(), &[]);
        assert_eq!(report["faults"], serde_json::json!([]));
        let group = report["groups"][0].as_object().expect("a group object");
        assert!(group.contains_key("detail"));
        assert!(group.contains_key("canary"));
    }
}
