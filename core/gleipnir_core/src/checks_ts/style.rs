//! Style and complexity checks for TypeScript source.
//!
//! Checks: function_length, param_count, nesting_depth, no_underscore_prefix,
//! no_single_letter_names, no_numbered_suffixes, short_param_names.

use crate::numbered_names;
use crate::parsing::{find_nodes_by_type, node_line, node_text};
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
// TypeScript function detection
// -------------------------------------------------------------------------

/// Node kinds that represent functions in TypeScript AST.
const TS_FUNCTION_KINDS: &[&str] = &[
    "function_declaration",
    "method_definition",
    "arrow_function",
];

/// Get function name from a TypeScript function node.
fn ts_func_name<'a>(node: tree_sitter::Node<'a>, source: &'a [u8]) -> &'a str {
    // function_declaration and method_definition have a "name" field
    if let Some(name_node) = node.child_by_field_name("name") {
        return node_text(name_node, source);
    }
    // arrow_function: check if parent is variable_declarator with a name
    if node.kind() == "arrow_function" {
        if let Some(parent) = node.parent() {
            if parent.kind() == "variable_declarator" {
                if let Some(name_node) = parent.child_by_field_name("name") {
                    return node_text(name_node, source);
                }
            }
        }
    }
    "<anonymous>"
}

/// Extract parameter names from a TypeScript function's parameters.
fn ts_param_names<'a>(
    node: tree_sitter::Node<'a>,
    source: &'a [u8],
) -> Vec<(usize, &'a str)> {
    let params_node = match node.child_by_field_name("parameters") {
        Some(n) => n,
        None => return Vec::new(),
    };

    let mut results = Vec::new();
    let mut cursor = params_node.walk();
    for param in params_node.named_children(&mut cursor) {
        match param.kind() {
            // TypeScript: required_parameter, optional_parameter
            "required_parameter" | "optional_parameter" => {
                if let Some(pattern) = param.child_by_field_name("pattern") {
                    let name = node_text(pattern, source);
                    if name != "this" {
                        results.push((node_line(param), name));
                    }
                }
            }
            // Plain JS: identifier as parameter
            "identifier" => {
                let name = node_text(param, source);
                results.push((node_line(param), name));
            }
            // Destructuring patterns — skip naming checks
            "object_pattern" | "array_pattern" => {}
            _ => {}
        }
    }
    results
}

/// Collect let/const binding names inside a function body.
fn ts_let_names<'a>(
    func_node: tree_sitter::Node<'a>,
    source: &'a [u8],
) -> Vec<(usize, &'a str)> {
    let body = match func_node.child_by_field_name("body") {
        Some(b) => b,
        None => return Vec::new(),
    };
    let mut results = Vec::new();
    for node in find_nodes_by_type(body, "variable_declarator") {
        if let Some(name_node) = node.child_by_field_name("name") {
            if name_node.kind() == "identifier" {
                let name = node_text(name_node, source);
                results.push((node_line(node), name));
            }
        }
    }
    results
}

// -------------------------------------------------------------------------
// function_length
// -------------------------------------------------------------------------

fn count_code_lines_ts(func_node: tree_sitter::Node, lines: &[&str]) -> usize {
    let start_row = func_node.start_position().row;
    let end_row = func_node.end_position().row;
    let mut count = 0;

    for row in (start_row + 1)..=end_row.min(lines.len().saturating_sub(1)) {
        let stripped = lines[row].trim();
        if stripped.is_empty()
            || stripped.starts_with("//")
            || stripped.starts_with("/*")
            || stripped.starts_with("*")
            || stripped == "{"
            || stripped == "}"
        {
            continue;
        }
        count += 1;
    }
    count
}

pub fn check_function_length(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for kind in TS_FUNCTION_KINDS {
        for func_node in find_nodes_by_type(source.tree.root_node(), kind) {
            let name = ts_func_name(func_node, source.source_bytes);
            let code_lines = count_code_lines_ts(func_node, &source.lines);
            if code_lines > config.max_function_lines {
                violations.push(violation(
                    node_line(func_node),
                    format!("{name}() too long"),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// param_count
// -------------------------------------------------------------------------

pub fn check_param_count(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for kind in TS_FUNCTION_KINDS {
        for func_node in find_nodes_by_type(source.tree.root_node(), kind) {
            let name = ts_func_name(func_node, source.source_bytes);
            let params = ts_param_names(func_node, source.source_bytes);
            if params.len() > config.max_function_params {
                violations.push(violation(
                    node_line(func_node),
                    format!("{name}() has too many parameters"),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// nesting_depth
// -------------------------------------------------------------------------

const TS_NESTING_TYPES: &[&str] = &[
    "if_statement",
    "for_statement",
    "for_in_statement",
    "while_statement",
    "do_statement",
    "switch_case",
    "try_statement",
];

fn compute_max_nesting_ts(node: tree_sitter::Node, current: usize) -> usize {
    let mut best = current;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if TS_NESTING_TYPES.contains(&child.kind()) {
            best = best.max(compute_max_nesting_ts(child, current + 1));
        } else {
            best = best.max(compute_max_nesting_ts(child, current));
        }
    }
    best
}

pub fn check_nesting_depth(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for kind in TS_FUNCTION_KINDS {
        for func_node in find_nodes_by_type(source.tree.root_node(), kind) {
            let name = ts_func_name(func_node, source.source_bytes);
            let depth = compute_max_nesting_ts(func_node, 0);
            if depth > config.max_nesting_depth {
                violations.push(violation(
                    node_line(func_node),
                    format!("{name}() nested too deeply"),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// Naming helpers
// -------------------------------------------------------------------------

const SINGLE_LETTER_ALLOWLIST: &[&str] = &["i", "j", "k", "_", "e", "$"];

const SHORT_PARAM_ALLOWLIST: &[&str] = &[
    "id", "db", "ok", "fn", "tx", "rx", "ui", "el", "ev",
    "cb", "on", "to",
];

fn is_bad_numbered_name(name: &str) -> bool {
    numbered_names::typescript_numbered().is_match(name)
        && !numbered_names::typescript_allowed().is_match(name)
}

// -------------------------------------------------------------------------
// no_underscore_prefix
// -------------------------------------------------------------------------

pub fn check_no_underscore_prefix(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for kind in TS_FUNCTION_KINDS {
        for func_node in find_nodes_by_type(source.tree.root_node(), kind) {
            let name = ts_func_name(func_node, source.source_bytes);

            // Check function name
            if name.starts_with('_') && name.len() > 1 {
                violations.push(violation(
                    node_line(func_node),
                    format!("function '{name}' uses underscore prefix"),
                ));
            }

            // Check parameters
            for (line, pname) in ts_param_names(func_node, source.source_bytes) {
                if pname.starts_with('_') && pname.len() > 1 {
                    violations.push(violation(
                        line,
                        format!("parameter '{pname}' uses underscore prefix in {name}()"),
                    ));
                }
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_single_letter_names
// -------------------------------------------------------------------------

pub fn check_no_single_letter_names(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for kind in TS_FUNCTION_KINDS {
        for func_node in find_nodes_by_type(source.tree.root_node(), kind) {
            let name = ts_func_name(func_node, source.source_bytes);

            for (line, pname) in ts_param_names(func_node, source.source_bytes) {
                if pname.len() == 1 && !SINGLE_LETTER_ALLOWLIST.contains(&pname) {
                    violations.push(violation(
                        line,
                        format!("single-letter parameter '{pname}' in {name}()"),
                    ));
                }
            }

            for (line, var_name) in ts_let_names(func_node, source.source_bytes) {
                if var_name.len() == 1 && !SINGLE_LETTER_ALLOWLIST.contains(&var_name) {
                    violations.push(violation(
                        line,
                        format!("single-letter variable '{var_name}' in {name}()"),
                    ));
                }
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

    for kind in TS_FUNCTION_KINDS {
        for func_node in find_nodes_by_type(source.tree.root_node(), kind) {
            let name = ts_func_name(func_node, source.source_bytes);

            for (line, pname) in ts_param_names(func_node, source.source_bytes) {
                if is_bad_numbered_name(pname) {
                    violations.push(violation(
                        line,
                        format!("numbered parameter '{pname}' in {name}()"),
                    ));
                }
            }

            for (line, var_name) in ts_let_names(func_node, source.source_bytes) {
                if is_bad_numbered_name(var_name) {
                    violations.push(violation(
                        line,
                        format!("numbered variable '{var_name}' in {name}()"),
                    ));
                }
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

    for kind in TS_FUNCTION_KINDS {
        for func_node in find_nodes_by_type(source.tree.root_node(), kind) {
            let name = ts_func_name(func_node, source.source_bytes);

            for (line, pname) in ts_param_names(func_node, source.source_bytes) {
                if pname.starts_with('_') {
                    continue;
                }
                if pname.len() < min_len && !SHORT_PARAM_ALLOWLIST.contains(&pname) {
                    violations.push(violation(
                        line,
                        format!("short parameter '{pname}' in {name}()"),
                    ));
                }
            }
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::build_parsed_source_typescript;
    use crate::structures::FileKind;

    fn parse(code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source_typescript("/test/file.ts", source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside, &crate::STATISTICS)
    }

    // -- function_length --

    #[test]
    fn short_function_ok() {
        let parsed = parse("function foo() { return 1; }");
        let violations = check_function_length(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn long_function_caught() {
        let mut code = String::from("function foo() {\n");
        for i in 0..55 {
            code.push_str(&format!("  const x{i} = {i};\n"));
        }
        code.push_str("}\n");
        let parsed = parse(&code);
        let violations = check_function_length(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("foo"));
    }

    #[test]
    fn arrow_function_length() {
        let mut code = String::from("const foo = () => {\n");
        for i in 0..55 {
            code.push_str(&format!("  const x{i} = {i};\n"));
        }
        code.push_str("};\n");
        let parsed = parse(&code);
        let violations = check_function_length(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("foo"));
    }

    // -- param_count --

    #[test]
    fn few_params_ok() {
        let parsed = parse("function foo(a: number, b: number, c: number) {}");
        let violations = check_param_count(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn too_many_params_caught() {
        let parsed = parse(
            "function foo(a: number, b: number, c: number, d: number, e: number, f: number) {}",
        );
        let violations = check_param_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- nesting_depth --

    #[test]
    fn shallow_nesting_ok() {
        let parsed = parse("function foo() { if (true) { return; } }");
        let violations = check_nesting_depth(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn deep_nesting_caught() {
        let code = r#"
function foo() {
    if (true) {
        for (let i = 0; i < 10; i++) {
            if (i > 0) {
                while (true) {
                    if (i > 5) {
                        break;
                    }
                }
            }
        }
    }
}
"#;
        let parsed = parse(code);
        let violations = check_nesting_depth(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- no_underscore_prefix --

    #[test]
    fn underscore_function_caught() {
        let parsed = parse("function _helper() {}");
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("_helper"));
    }

    #[test]
    fn normal_function_ok() {
        let parsed = parse("function helper() {}");
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_single_letter_names --

    #[test]
    fn single_letter_param_caught() {
        let parsed = parse("function foo(x: number) {}");
        let violations = check_no_single_letter_names(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn event_param_ok() {
        let parsed = parse("function foo(e: Event) {}");
        let violations = check_no_single_letter_names(&parsed, &default_config());
        assert!(violations.is_empty()); // 'e' is allowlisted for event handlers
    }

    #[test]
    fn loop_index_ok() {
        let parsed = parse("function foo() { for (let i = 0; i < 10; i++) {} }");
        let violations = check_no_single_letter_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_numbered_suffixes --

    #[test]
    fn numbered_var_caught() {
        let parsed = parse("function foo() { const result1 = 1; }");
        let violations = check_no_numbered_suffixes(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("result1"));
    }

    #[test]
    fn allowed_numbered_ok() {
        let parsed = parse("function foo() { const sha256 = 'abc'; }");
        let violations = check_no_numbered_suffixes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- short_param_names --

    #[test]
    fn short_param_caught() {
        let parsed = parse("function foo(ab: number) {}");
        let violations = check_short_param_names(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn allowlisted_short_ok() {
        let parsed = parse("function foo(id: string, el: Element) {}");
        let violations = check_short_param_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn long_param_ok() {
        let parsed = parse("function foo(userName: string) {}");
        let violations = check_short_param_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }
}
