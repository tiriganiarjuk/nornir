//! Style and complexity checks.
//!
//! Checks: function_length, no_underscore_prefix, param_count,
//! nesting_depth, no_none_returns, no_throwaway_assignment,
//! no_single_letter_names, no_numbered_suffixes, short_param_names.

use crate::numbered_names;
use crate::parsing::{find_nodes_by_type, node_field, node_line, node_text, walk_tree};
use crate::structures::{CheckConfig, ParsedSource, Severity, Violation};

fn violation(line: usize, message: String) -> Violation {
    Violation {
        line,
        check_name: String::new(),
        severity: Severity::Warning,
        message,
        detail: String::new(),
        signal: String::new(),
        direction: String::new(),
        canary: String::new(),
    }
}

// -------------------------------------------------------------------------
// function_length
// -------------------------------------------------------------------------

fn docstring_end_row(body: tree_sitter::Node) -> Option<usize> {
    let mut cursor = body.walk();
    let first = body.named_children(&mut cursor).next()?;
    if first.kind() != "expression_statement" {
        return None;
    }
    let mut inner_cursor = first.walk();
    for child in first.named_children(&mut inner_cursor) {
        if child.kind() == "string" {
            return Some(first.end_position().row);
        }
    }
    None
}

fn count_code_lines(func_node: tree_sitter::Node, lines: &[&str]) -> usize {
    let start_row = func_node.start_position().row;
    let end_row = func_node.end_position().row;
    let body = node_field(func_node, "body");
    let doc_end = body.and_then(docstring_end_row);

    let mut count = 0;
    for row in start_row..=end_row.min(lines.len().saturating_sub(1)) {
        if row == start_row {
            continue;
        }
        if let Some(de) = doc_end {
            if row <= de {
                continue;
            }
        }
        let stripped = lines[row].trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            continue;
        }
        count += 1;
    }
    count
}

pub fn check_function_length(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };

        if func_name.starts_with("test_") || func_name.starts_with("__") {
            continue;
        }

        let code_lines = count_code_lines(func_node, &source.lines);
        if code_lines > config.max_function_lines {
            // Fuzzy — no counts
            violations.push(violation(
                node_line(func_node),
                format!("{func_name}() too long"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_underscore_prefix
// -------------------------------------------------------------------------

fn is_underscore_violation(name: &str) -> bool {
    if !name.starts_with('_') {
        return false;
    }
    if name == "_" {
        return false;
    }
    if name.starts_with("__") && name.ends_with("__") {
        return false;
    }
    // Single-letter type var like _T
    if name.len() == 2 && name.as_bytes()[1].is_ascii_uppercase() {
        return false;
    }
    true
}

pub fn check_no_underscore_prefix(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };
        if is_underscore_violation(name) {
            violations.push(violation(
                node_line(func_node),
                format!("function '{name}' uses underscore prefix"),
            ));
        }
    }

    for class_node in find_nodes_by_type(source.tree.root_node(), "class_definition") {
        let name = match node_field(class_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };
        if is_underscore_violation(name) {
            violations.push(violation(
                node_line(class_node),
                format!("class '{name}' uses underscore prefix"),
            ));
        }
    }

    for assign_node in find_nodes_by_type(source.tree.root_node(), "assignment") {
        let left = match node_field(assign_node, "left") {
            Some(n) if n.kind() == "identifier" => n,
            _ => continue,
        };
        let name = node_text(left, source.source_bytes);
        if is_underscore_violation(name) {
            violations.push(violation(
                node_line(assign_node),
                format!("variable '{name}' uses underscore prefix"),
            ));
        }
    }

    violations
}

// -------------------------------------------------------------------------
// param_count
// -------------------------------------------------------------------------

fn extract_param_name<'a>(param: tree_sitter::Node<'a>, source: &'a [u8]) -> &'a str {
    match param.kind() {
        "identifier" => node_text(param, source),
        "typed_parameter" | "default_parameter" | "typed_default_parameter" => {
            let mut cursor = param.walk();
            let first = param.named_children(&mut cursor).next();
            match first {
                Some(n) if n.kind() == "identifier" => node_text(n, source),
                _ => "",
            }
        }
        _ => "",
    }
}

/// Whether a parameter is `*args` or `**kwargs`, annotated or not.
fn is_splat_parameter(param: tree_sitter::Node) -> bool {
    let is_splat = |node: tree_sitter::Node| matches!(node.kind(), "list_splat_pattern" | "dictionary_splat_pattern");
    is_splat(param) || (param.kind() == "typed_parameter" && param.named_child(0).is_some_and(is_splat))
}

pub fn check_param_count(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };

        if func_name.starts_with("test_") || func_name.starts_with("__") {
            continue;
        }

        let params_node = match node_field(func_node, "parameters") {
            Some(n) => n,
            None => continue,
        };

        let mut count = 0;
        let mut cursor = params_node.walk();
        for param in params_node.named_children(&mut cursor) {
            // *args and **kwargs carry any number of inputs: each is one parameter at least.
            if is_splat_parameter(param) {
                count += 1;
                continue;
            }
            let name = extract_param_name(param, source.source_bytes);
            if name == "self" || name == "cls" || name.is_empty() {
                continue;
            }
            count += 1;
        }

        if count > config.max_function_params {
            // Fuzzy — no counts
            violations.push(violation(
                node_line(func_node),
                format!("{func_name}() has too many parameters"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// nesting_depth
// -------------------------------------------------------------------------

const NESTING_TYPES: &[&str] = &[
    "if_statement",
    "for_statement",
    "while_statement",
    "with_statement",
    "try_statement",
    "except_clause",
    "match_statement",
];

fn compute_max_nesting(node: tree_sitter::Node, current: usize) -> usize {
    let mut best = current;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if NESTING_TYPES.contains(&child.kind()) {
            best = best.max(compute_max_nesting(child, current + 1));
        } else {
            best = best.max(compute_max_nesting(child, current));
        }
    }
    best
}

pub fn check_nesting_depth(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };

        if func_name.starts_with("test_") {
            continue;
        }

        let depth = compute_max_nesting(func_node, 0);
        if depth > config.max_nesting_depth {
            // Fuzzy — no counts
            violations.push(violation(
                node_line(func_node),
                format!("{func_name}() nested too deeply"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_none_returns
// -------------------------------------------------------------------------

/// Whether a function body hands a value back: a `return` with a value, or a `yield`.
/// A nested function, class or lambda answers for its own.
fn hands_back_a_value(node: tree_sitter::Node) -> bool {
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).any(|child| match child.kind() {
        "function_definition" | "class_definition" | "lambda" => false,
        "return_statement" => child.named_child_count() > 0,
        "yield" => true,
        _ => hands_back_a_value(child),
    });
    found
}

pub fn check_no_none_returns(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };

        if func_name.starts_with("test_") || func_name.starts_with("__") {
            continue;
        }

        let return_type = match node_field(func_node, "return_type") {
            Some(n) => n,
            None => {
                // With no annotation the body is the only witness: a function that hands
                // back nothing returns None whether or not it says so.
                let hands_back = node_field(func_node, "body").is_some_and(hands_back_a_value);
                if !hands_back {
                    violations.push(violation(node_line(func_node), format!("{func_name}() returns nothing")));
                }
                continue;
            }
        };

        let rt_text = node_text(return_type, source.source_bytes).trim();
        if rt_text == "None" {
            violations.push(violation(
                node_line(func_node),
                format!("{func_name}() returns None"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_throwaway_assignment
// -------------------------------------------------------------------------

pub fn check_no_throwaway_assignment(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "assignment") {
        let left = match node_field(node, "left") {
            Some(n) if n.kind() == "identifier" => n,
            _ => continue,
        };
        if node_text(left, source.source_bytes) == "_" {
            violations.push(violation(
                node_line(node),
                "throwaway assignment discards return value".to_string(),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// Naming quality helpers
// -------------------------------------------------------------------------

const SINGLE_LETTER_ALLOWLIST: &[&str] = &["i", "j", "k", "_"];

const SHORT_PARAM_ALLOWLIST: &[&str] = &[
    "self", "cls", "id", "db", "ok", "os", "io", "fn", "fp", "fh", "ip", "tx", "rx", "ui", "pk",
    "op",
];

fn func_param_names<'a>(
    func_node: tree_sitter::Node<'a>,
    source: &'a [u8],
) -> Vec<(usize, &'a str)> {
    let params_node = match node_field(func_node, "parameters") {
        Some(n) => n,
        None => return Vec::new(),
    };
    let mut results = Vec::new();
    let mut cursor = params_node.walk();
    for param in params_node.named_children(&mut cursor) {
        if param.kind() == "list_splat_pattern" || param.kind() == "dictionary_splat_pattern" {
            continue;
        }
        let name = extract_param_name(param, source);
        if !name.is_empty() && name != "self" && name != "cls" {
            results.push((node_line(param), name));
        }
    }
    results
}

fn names_from_target<'a>(
    target: tree_sitter::Node<'a>,
    source: &'a [u8],
) -> Vec<(usize, &'a str)> {
    let mut results = Vec::new();
    if target.kind() == "identifier" {
        results.push((node_line(target), node_text(target, source)));
    } else if target.kind() == "pattern_list" {
        let mut cursor = target.walk();
        for child in target.named_children(&mut cursor) {
            if child.kind() == "identifier" {
                results.push((node_line(child), node_text(child, source)));
            }
        }
    }
    results
}

/// Extract alias names from a with_clause node (e.g., `with open(f) as handle:`).
/// AST path: with_clause → with_item → as_pattern → as_pattern_target → identifier
fn names_from_with_clause<'a>(
    node: tree_sitter::Node<'a>,
    source: &'a [u8],
) -> Vec<(usize, &'a str)> {
    let mut results = Vec::new();
    for target_node in walk_tree(node) {
        if target_node.kind() == "as_pattern_target" {
            results.extend(names_from_target_recursive(target_node, source));
        }
    }
    results
}

/// Extract identifier names from a node, recursing into children.
/// Handles wrapper nodes like as_pattern_target that contain identifiers.
fn names_from_target_recursive<'a>(
    node: tree_sitter::Node<'a>,
    source: &'a [u8],
) -> Vec<(usize, &'a str)> {
    if node.kind() == "identifier" {
        return vec![(node_line(node), node_text(node, source))];
    }
    let mut results = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "identifier" {
            results.push((node_line(child), node_text(child, source)));
        }
    }
    results
}

fn collect_body_target_names<'a>(
    func_node: tree_sitter::Node<'a>,
    source: &'a [u8],
) -> Vec<(usize, &'a str)> {
    let body = match node_field(func_node, "body") {
        Some(b) => b,
        None => return Vec::new(),
    };
    let mut results = Vec::new();
    for node in walk_tree(body) {
        match node.kind() {
            "assignment" => {
                if let Some(left) = node_field(node, "left") {
                    results.extend(names_from_target(left, source));
                }
            }
            "for_statement" | "for_in_clause" => {
                if let Some(left) = node_field(node, "left") {
                    results.extend(names_from_target(left, source));
                }
            }
            "with_clause" => results.extend(names_from_with_clause(node, source)),
            _ => {}
        }
    }
    results
}

fn is_bad_numbered_name(name: &str) -> bool {
    numbered_names::python_numbered().is_match(name) && !numbered_names::python_allowed().is_match(name)
}

// -------------------------------------------------------------------------
// no_single_letter_names
// -------------------------------------------------------------------------

pub fn check_no_single_letter_names(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };
        if func_name.starts_with("test_") {
            continue;
        }

        for (line, name) in func_param_names(func_node, source.source_bytes) {
            if name.len() == 1 && !SINGLE_LETTER_ALLOWLIST.contains(&name) {
                violations.push(violation(
                    line,
                    format!("single-letter parameter '{name}' in {func_name}()"),
                ));
            }
        }

        for (line, name) in collect_body_target_names(func_node, source.source_bytes) {
            if name.len() == 1 && !SINGLE_LETTER_ALLOWLIST.contains(&name) {
                violations.push(violation(
                    line,
                    format!("single-letter variable '{name}' in {func_name}()"),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_numbered_suffixes
// -------------------------------------------------------------------------

pub fn check_no_numbered_suffixes(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };

        for (line, name) in func_param_names(func_node, source.source_bytes) {
            if is_bad_numbered_name(name) {
                violations.push(violation(
                    line,
                    format!("numbered parameter '{name}' in {func_name}()"),
                ));
            }
        }

        for (line, name) in collect_body_target_names(func_node, source.source_bytes) {
            if is_bad_numbered_name(name) {
                violations.push(violation(
                    line,
                    format!("numbered variable '{name}' in {func_name}()"),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// short_param_names
// -------------------------------------------------------------------------

pub fn check_short_param_names(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    let min_len = config.min_param_length;

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };

        for (line, name) in func_param_names(func_node, source.source_bytes) {
            if name.starts_with('_') {
                continue;
            }
            if name.len() < min_len && !SHORT_PARAM_ALLOWLIST.contains(&name) {
                // Fuzzy — no counts
                violations.push(violation(
                    line,
                    format!("short parameter '{name}' in {func_name}()"),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// short_local_names
// -------------------------------------------------------------------------

/// Allowlist for short local variable names that are idiomatic or conventional.
const SHORT_LOCAL_ALLOWLIST: &[&str] = &[
    "db", "ok", "io", "fn", "fp", "fh", "ip", "tx", "rx", "ui", "pk", "op",
];

pub fn check_short_local_names(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    let min_len = config.min_param_length;

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };

        for (line, name) in collect_body_target_names(func_node, source.source_bytes) {
            if name.starts_with('_') || name.len() <= 1 {
                // Single-letter names are caught by no_single_letter_names
                continue;
            }
            if name.len() < min_len && !SHORT_LOCAL_ALLOWLIST.contains(&name) {
                violations.push(violation(
                    line,
                    format!("short local variable '{name}' in {func_name}()"),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// cyclomatic_complexity (radon parity)
// -------------------------------------------------------------------------

/// Compute cyclomatic complexity for a function node.
///
/// Counts decision points matching radon's algorithm:
/// if, elif, for, while, except, boolean and/or, ternary,
/// comprehension for/if, match case, assert, loop/try else.
///
/// Nested function definitions are excluded — they get their own CC score.
pub fn cyclomatic_complexity(func_node: tree_sitter::Node, source: &[u8]) -> usize {
    let body = match node_field(func_node, "body") {
        Some(b) => b,
        None => return 1,
    };
    let mut complexity: usize = 1;
    cc_walk(body, source, &mut complexity);
    complexity
}

fn cc_walk(node: tree_sitter::Node, source: &[u8], complexity: &mut usize) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            // Skip nested functions — they have their own CC
            "function_definition" | "async_function_definition" => continue,

            "if_statement" => *complexity += 1,
            "elif_clause" => *complexity += 1,
            "conditional_expression" => *complexity += 1,

            "for_statement" | "async_for_statement" => {
                *complexity += 1;
                if has_else_clause(child) {
                    *complexity += 1;
                }
            }

            "while_statement" => {
                *complexity += 1;
                if has_else_clause(child) {
                    *complexity += 1;
                }
            }

            "except_clause" => *complexity += 1,

            // else clause on try_statement (not on if — that's not a decision point)
            "else_clause" => {
                if let Some(parent) = child.parent() {
                    if parent.kind() == "try_statement" {
                        *complexity += 1;
                    }
                }
            }

            "boolean_operator" => {
                let op = node_field(child, "operator")
                    .map(|n| node_text(n, source))
                    .unwrap_or("");
                if op == "and" || op == "or" {
                    *complexity += 1;
                }
            }

            // Comprehension: for_in_clause adds +1, if_clause adds +1
            "for_in_clause" => *complexity += 1,
            "if_clause" => *complexity += 1,

            // Match/case: each case arm adds +1
            "case_clause" => {
                // Wildcard `case _:` doesn't add — it's the default
                if !is_wildcard_case(child, source) {
                    *complexity += 1;
                }
            }

            "assert_statement" => *complexity += 1,

            _ => {}
        }
        cc_walk(child, source, complexity);
    }
}

fn has_else_clause(node: tree_sitter::Node) -> bool {
    let mut cursor = node.walk();
    let result = node.named_children(&mut cursor)
        .any(|child| child.kind() == "else_clause");
    result
}

fn is_wildcard_case(case_node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut cursor = case_node.walk();
    for child in case_node.named_children(&mut cursor) {
        if child.kind() == "case_pattern" {
            let text = node_text(child, source).trim().to_string();
            return text == "_";
        }
    }
    false
}

// -------------------------------------------------------------------------
// check_v2_cc_level — gravity and ceiling enforcement
// -------------------------------------------------------------------------

/// Check that function cyclomatic complexity matches the v2 level.
///
/// Gravity: CC below the level minimum → function must move down.
/// Ceiling: CC above the level maximum → function must move up.
pub fn check_v2_cc_level(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let min_cc = config.min_cc;
    let max_cc = config.max_cc;

    // Levels without CC bounds (Structure, EntryPoint, Outside) have min=0/max=MAX
    if min_cc == 0 && max_cc == usize::MAX {
        return Vec::new();
    }

    let mut violations = Vec::new();
    let func_kinds = ["function_definition", "async_function_definition"];

    for kind in &func_kinds {
        for func_node in find_nodes_by_type(source.tree.root_node(), kind) {
            let func_name = node_field(func_node, "name")
                .map(|n| node_text(n, source.source_bytes))
                .unwrap_or("<unknown>");

            let cc = cyclomatic_complexity(func_node, source.source_bytes);

            if cc < min_cc {
                violations.push(violation(
                    node_line(func_node),
                    format!(
                        "{func_name}() gravity violation: too simple for this level — belongs at a lower level",
                    ),
                ));
            } else if cc > max_cc {
                violations.push(violation(
                    node_line(func_node),
                    format!(
                        "{func_name}() ceiling violation: too complex for this level — belongs at a higher level or needs decomposition",
                    ),
                ));
            }
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

    fn parse_with_path(code: &str, path: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        let path: &'static str = Box::leak(path.to_string().into_boxed_str());
        build_parsed_source(path, source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside, &crate::STATISTICS)
    }

    // -- function_length --

    #[test]
    fn short_function_ok() {
        let parsed = parse("def foo():\n    return 1\n");
        let violations = check_function_length(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn long_function_caught() {
        // Generate a function with 51+ code lines (config max is 50 for Outside)
        let mut code = String::from("def foo():\n");
        for i in 0..55 {
            code.push_str(&format!("    x{i} = {i}\n"));
        }
        let parsed = parse(&code);
        let violations = check_function_length(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("foo"));
        // Fuzzy — no line count
        assert!(!violations[0].message.contains("55"));
    }

    #[test]
    fn test_function_skipped() {
        let mut code = String::from("def test_big():\n");
        for i in 0..55 {
            code.push_str(&format!("    x{i} = {i}\n"));
        }
        let parsed = parse(&code);
        let violations = check_function_length(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn docstring_excluded_from_count() {
        let mut code = String::from("def foo():\n    \"\"\"Long docstring.\n");
        for _ in 0..20 {
            code.push_str("    Line of docstring.\n");
        }
        code.push_str("    \"\"\"\n");
        // Add only a few code lines after docstring
        for i in 0..5 {
            code.push_str(&format!("    x{i} = {i}\n"));
        }
        let parsed = parse(&code);
        let violations = check_function_length(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_underscore_prefix --

    #[test]
    fn underscore_function_caught() {
        let parsed = parse("def _helper():\n    pass\n");
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("_helper"));
    }

    #[test]
    fn dunder_ok() {
        let parsed = parse("def __init__(self):\n    pass\n");
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn single_underscore_ok() {
        let parsed = parse("_ = compute()\n");
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn underscore_variable_caught() {
        let parsed = parse("_cache = {}\n");
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("_cache"));
    }

    // -- param_count --

    #[test]
    fn few_params_ok() {
        let parsed = parse("def foo(a, b, c):\n    pass\n");
        let violations = check_param_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 3 params, max 5
    }

    #[test]
    fn too_many_params_caught() {
        let parsed = parse("def foo(a, b, c, d, e, f, g):\n    pass\n");
        let violations = check_param_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("foo"));
        // Fuzzy — no counts
        assert!(!violations[0].message.contains("7"));
    }

    #[test]
    fn self_excluded() {
        let parsed = parse("def foo(self, a, b, c, d, e):\n    pass\n");
        let violations = check_param_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 5 params after excluding self
    }

    #[test]
    fn splat_parameters_are_counted() {
        // The edge: *args and **kwargs carried any number of inputs and counted as none.
        for code in [
            "def foo(a, b, c, d, e, *rest):\n    pass\n",
            "def foo(a, b, c, d, e, **options):\n    pass\n",
            "def foo(a, b, c, d, *rest, **options):\n    pass\n",
            "def foo(a, b, c, d, e, *rest: int):\n    pass\n",
            "def foo(a, b, c, d, e, **options: str):\n    pass\n",
        ] {
            assert_eq!(check_param_count(&parse(code), &default_config()).len(), 1, "{code}");
        }
    }

    #[test]
    fn the_keyword_only_marker_is_not_a_parameter() {
        let parsed = parse("def foo(a, b, *, c, d, e):\n    pass\n");
        assert!(check_param_count(&parsed, &default_config()).is_empty());
    }

    // -- nesting_depth --

    #[test]
    fn nesting_through_match_is_counted() {
        // The edge: a match statement was not a level of nesting.
        let code = "\
def foo(value):
    if value:
        for item in value:
            match item:
                case 1:
                    if item:
                        while True:
                            pass
";
        let violations = check_nesting_depth(&parse(code), &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn shallow_nesting_ok() {
        let parsed = parse("def foo():\n    if True:\n        return 1\n");
        let violations = check_nesting_depth(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn deep_nesting_caught() {
        let code = "\
def foo():
    if True:
        for x in range(10):
            if x > 0:
                while True:
                    if x > 5:
                        pass
";
        let parsed = parse(code);
        let violations = check_nesting_depth(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("foo"));
    }

    // -- no_none_returns --

    #[test]
    fn none_return_caught() {
        let parsed = parse("def foo() -> None:\n    pass\n");
        let violations = check_no_none_returns(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("foo"));
    }

    #[test]
    fn int_return_ok() {
        let parsed = parse("def foo() -> int:\n    return 1\n");
        let violations = check_no_none_returns(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn a_function_with_no_return_annotation_that_returns_nothing_caught() {
        // The edge: leaving `-> None` off silenced the check.
        for code in [
            "def save(page):\n    write(page)\n",
            "def save(page):\n    write(page)\n    return\n",
            "def save(page):\n    def inner():\n        return 1\n    write(page)\n",
        ] {
            let violations = check_no_none_returns(&parse(code), &default_config());
            assert_eq!(violations.len(), 1, "{code}");
            assert!(violations[0].message.contains("save"), "{code}");
        }
    }

    #[test]
    fn a_function_with_no_return_annotation_that_returns_a_value_ok() {
        for code in [
            "def total(page):\n    return page.count\n",
            "def total(page):\n    if page:\n        return 1\n    return 0\n",
            "def pages(site):\n    yield site.first\n",
        ] {
            assert!(check_no_none_returns(&parse(code), &default_config()).is_empty(), "{code}");
        }
    }

    #[test]
    fn dunder_none_return_ok() {
        let parsed = parse("def __init__(self) -> None:\n    pass\n");
        let violations = check_no_none_returns(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_throwaway_assignment --

    #[test]
    fn throwaway_caught() {
        let parsed = parse("_ = compute()\n");
        let violations = check_no_throwaway_assignment(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn normal_assignment_ok() {
        let parsed = parse("result = compute()\n");
        let violations = check_no_throwaway_assignment(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_single_letter_names --

    #[test]
    fn single_letter_param_caught() {
        let parsed = parse("def foo(x):\n    pass\n");
        let violations = check_no_single_letter_names(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("'x'"));
    }

    #[test]
    fn loop_index_ok() {
        let parsed = parse("def foo():\n    for i in range(10):\n        pass\n");
        let violations = check_no_single_letter_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn single_letter_local_caught() {
        let parsed = parse("def foo():\n    x = 1\n");
        let violations = check_no_single_letter_names(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("variable"));
    }

    // -- no_numbered_suffixes --

    #[test]
    fn numbered_param_caught() {
        let parsed = parse("def foo(result1):\n    pass\n");
        let violations = check_no_numbered_suffixes(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("result1"));
    }

    #[test]
    fn allowed_numbered_ok() {
        let parsed = parse("def foo(sha256):\n    pass\n");
        let violations = check_no_numbered_suffixes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn utf8_ok() {
        let parsed = parse("def foo(utf8):\n    pass\n");
        let violations = check_no_numbered_suffixes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn numbered_local_caught() {
        let parsed = parse("def foo():\n    data1 = 1\n");
        let violations = check_no_numbered_suffixes(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("data1"));
    }

    // -- short_param_names --

    #[test]
    fn short_param_caught() {
        let parsed = parse("def foo(ab):\n    pass\n");
        let violations = check_short_param_names(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("'ab'"));
    }

    #[test]
    fn allowlisted_short_ok() {
        let parsed = parse("def foo(id, db, ok):\n    pass\n");
        let violations = check_short_param_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn long_param_ok() {
        let parsed = parse("def foo(user_name):\n    pass\n");
        let violations = check_short_param_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn underscore_prefix_param_skipped() {
        let parsed = parse("def foo(_x):\n    pass\n");
        let violations = check_short_param_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- cyclomatic_complexity --

    #[test]
    fn cc_straight_line_is_1() {
        let parsed = parse("def foo():\n    x = 1\n    return x\n");
        let func = find_nodes_by_type(parsed.tree.root_node(), "function_definition")[0];
        assert_eq!(cyclomatic_complexity(func, parsed.source_bytes), 1);
    }

    #[test]
    fn cc_single_if_is_2() {
        let parsed = parse("def foo(x):\n    if x:\n        return 1\n    return 0\n");
        let func = find_nodes_by_type(parsed.tree.root_node(), "function_definition")[0];
        assert_eq!(cyclomatic_complexity(func, parsed.source_bytes), 2);
    }

    #[test]
    fn cc_if_elif_else_is_3() {
        let parsed = parse("def foo(x):\n    if x > 0:\n        return 1\n    elif x < 0:\n        return -1\n    else:\n        return 0\n");
        let func = find_nodes_by_type(parsed.tree.root_node(), "function_definition")[0];
        assert_eq!(cyclomatic_complexity(func, parsed.source_bytes), 3);
    }

    #[test]
    fn cc_for_loop_is_2() {
        let parsed = parse("def foo(xs):\n    for x in xs:\n        pass\n");
        let func = find_nodes_by_type(parsed.tree.root_node(), "function_definition")[0];
        assert_eq!(cyclomatic_complexity(func, parsed.source_bytes), 2);
    }

    #[test]
    fn cc_for_with_else_is_3() {
        let parsed = parse("def foo(xs):\n    for x in xs:\n        pass\n    else:\n        pass\n");
        let func = find_nodes_by_type(parsed.tree.root_node(), "function_definition")[0];
        assert_eq!(cyclomatic_complexity(func, parsed.source_bytes), 3);
    }

    #[test]
    fn cc_while_is_2() {
        let parsed = parse("def foo():\n    while True:\n        break\n");
        let func = find_nodes_by_type(parsed.tree.root_node(), "function_definition")[0];
        assert_eq!(cyclomatic_complexity(func, parsed.source_bytes), 2);
    }

    #[test]
    fn cc_except_handlers() {
        let parsed = parse("def foo():\n    try:\n        pass\n    except ValueError:\n        pass\n    except TypeError:\n        pass\n");
        let func = find_nodes_by_type(parsed.tree.root_node(), "function_definition")[0];
        assert_eq!(cyclomatic_complexity(func, parsed.source_bytes), 3);
    }

    #[test]
    fn cc_try_else_counts() {
        let parsed = parse("def foo():\n    try:\n        pass\n    except ValueError:\n        pass\n    else:\n        pass\n");
        let func = find_nodes_by_type(parsed.tree.root_node(), "function_definition")[0];
        assert_eq!(cyclomatic_complexity(func, parsed.source_bytes), 3);
    }

    #[test]
    fn cc_boolean_and_or() {
        let parsed = parse("def foo(a, b, c):\n    if a and b or c:\n        pass\n");
        let func = find_nodes_by_type(parsed.tree.root_node(), "function_definition")[0];
        // 1 (base) + 1 (if) + 1 (and) + 1 (or) = 4
        assert_eq!(cyclomatic_complexity(func, parsed.source_bytes), 4);
    }

    #[test]
    fn cc_ternary() {
        let parsed = parse("def foo(x):\n    return 1 if x else 0\n");
        let func = find_nodes_by_type(parsed.tree.root_node(), "function_definition")[0];
        assert_eq!(cyclomatic_complexity(func, parsed.source_bytes), 2);
    }

    #[test]
    fn cc_list_comprehension() {
        let parsed = parse("def foo(xs):\n    return [x for x in xs if x > 0]\n");
        let func = find_nodes_by_type(parsed.tree.root_node(), "function_definition")[0];
        // 1 (base) + 1 (for_in_clause) + 1 (if_clause) = 3
        assert_eq!(cyclomatic_complexity(func, parsed.source_bytes), 3);
    }

    #[test]
    fn cc_assert_counts() {
        let parsed = parse("def foo(x):\n    assert x > 0\n");
        let func = find_nodes_by_type(parsed.tree.root_node(), "function_definition")[0];
        assert_eq!(cyclomatic_complexity(func, parsed.source_bytes), 2);
    }

    #[test]
    fn cc_nested_function_excluded() {
        let parsed = parse("def outer():\n    def inner():\n        if True:\n            pass\n    return inner\n");
        let funcs = find_nodes_by_type(parsed.tree.root_node(), "function_definition");
        let outer = funcs[0];
        // outer has CC=1 — the inner function's if doesn't count
        assert_eq!(cyclomatic_complexity(outer, parsed.source_bytes), 1);
    }

    // -- check_v2_cc_level --

    fn v2_config(level: crate::structures::Level) -> CheckConfig {
        CheckConfig::for_v2(level, crate::structures::Zone::Pure, &crate::STATISTICS)
    }

    #[test]
    fn cc_ceiling_violation_in_primitive() {
        let parsed = parse_with_path(
            "def foo(x):\n    if x:\n        return 1\n    return 0\n",
            "/project/src/pkg/logic/pure/check/primitive.py",
        );
        let violations = check_v2_cc_level(&parsed, &v2_config(crate::structures::Level::L1));
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("ceiling"));
    }

    #[test]
    fn cc_no_gravity_for_cc1_in_simple() {
        let parsed = parse_with_path(
            "def foo():\n    return 1\n",
            "/project/src/pkg/logic/pure/check/simple.py",
        );
        let violations = check_v2_cc_level(&parsed, &v2_config(crate::structures::Level::L2));
        assert!(violations.is_empty());
    }

    #[test]
    fn cc_gravity_violation_in_composed() {
        let parsed = parse_with_path(
            "def foo(x):\n    if x:\n        return 1\n    return 0\n",
            "/project/src/pkg/logic/pure/check/composed.py",
        );
        let violations = check_v2_cc_level(&parsed, &v2_config(crate::structures::Level::L4));
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("gravity"));
    }

    #[test]
    fn cc_correct_in_simple() {
        let parsed = parse_with_path(
            "def foo(x):\n    if x:\n        return 1\n    return 0\n",
            "/project/src/pkg/logic/pure/check/simple.py",
        );
        let violations = check_v2_cc_level(&parsed, &v2_config(crate::structures::Level::L2));
        assert!(violations.is_empty());
    }

    #[test]
    fn cc_correct_in_composed() {
        let parsed = parse_with_path(
            "def foo(a, b, c, d):\n    if a:\n        if b:\n            return 1\n    elif c:\n        return 2\n    return 0\n",
            "/project/src/pkg/logic/pure/check/composed.py",
        );
        let violations = check_v2_cc_level(&parsed, &v2_config(crate::structures::Level::L4));
        assert!(violations.is_empty());
    }

    const HIGH_CC_FUNCTION: &str = "def foo(a, b, c, d):\n    if a:\n        if b:\n            return 1\n    elif c:\n        return 2\n    return 0\n";

    #[test]
    fn cc_outside_level_has_no_band() {
        let path = "/project/src/pkg/random_file.py";
        let classification = crate::classify::classify_file_v2(path);
        assert_eq!(classification.level, crate::structures::Level::Outside);
        let config = CheckConfig::for_v2(classification.level, classification.zone, &crate::STATISTICS);
        let parsed = parse_with_path(HIGH_CC_FUNCTION, path);
        assert!(check_v2_cc_level(&parsed, &config).is_empty());
    }

    #[test]
    fn cc_ceiling_violation_in_entry_point() {
        let path = "/project/src/pkg/cli.py";
        let classification = crate::classify::classify_file_v2(path);
        assert_eq!(classification.level, crate::structures::Level::L8);
        let config = CheckConfig::for_v2(classification.level, classification.zone, &crate::STATISTICS);
        let parsed = parse_with_path(HIGH_CC_FUNCTION, path);
        let violations = check_v2_cc_level(&parsed, &config);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("ceiling"));
    }

    #[test]
    fn cc_level_messages_carry_no_numbers() {
        let ceiling = parse_with_path(HIGH_CC_FUNCTION, "/project/src/pkg/logic/pure/check/primitive.py");
        let gravity = parse_with_path(
            "def foo():\n    return 1\n",
            "/project/src/pkg/logic/pure/check/composed.py",
        );
        let mut violations = check_v2_cc_level(&ceiling, &v2_config(crate::structures::Level::L1));
        violations.extend(check_v2_cc_level(&gravity, &v2_config(crate::structures::Level::L4)));
        assert_eq!(violations.len(), 2);
        assert!(violations[0].message.contains("foo") && violations[0].message.contains("ceiling violation"));
        assert!(violations[1].message.contains("foo") && violations[1].message.contains("gravity violation"));
        for viol in &violations {
            assert!(!viol.message.contains(|c: char| c.is_ascii_digit()), "{}", viol.message);
        }
    }
}
