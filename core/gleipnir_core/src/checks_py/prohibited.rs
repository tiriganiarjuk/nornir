//! Prohibited pattern checks.
//!
//! Checks: no_cast, no_overload, no_bare_except, no_broad_exceptions,
//! no_print, no_model_dump, no_future_annotations, init_files_empty,
//! no_dunder_all, no_nested_functions (+ no_closures),
//! no_general_lambda, no_default_factory, no_recursion.

use crate::parsing::{find_nodes_by_type, node_field, node_line, node_text};
use crate::structures::{CheckConfig, ParsedSource, Severity, Violation};

fn violation(line: usize, message: String) -> Violation {
    Violation {
        line,
        check_name: String::new(),
        severity: Severity::Error,
        message,
        detail: String::new(),
        signal: String::new(),
        direction: String::new(),
        canary: String::new(),
    }
}

// -------------------------------------------------------------------------
// no_cast
// -------------------------------------------------------------------------

fn is_cast_call(node: tree_sitter::Node, source: &[u8]) -> bool {
    let func = match node.child_by_field_name("function") {
        Some(f) => f,
        None => return false,
    };
    match func.kind() {
        "identifier" => node_text(func, source) == "cast",
        "attribute" => {
            func.child_by_field_name("attribute")
                .is_some_and(|a| node_text(a, source) == "cast")
        }
        _ => false,
    }
}

pub fn check_no_cast(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    for node in find_nodes_by_type(source.tree.root_node(), "call") {
        if is_cast_call(node, source.source_bytes) {
            violations.push(violation(node_line(node), "typing.cast() usage".to_string()));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_overload
// -------------------------------------------------------------------------

fn extract_decorator_name(decorator: tree_sitter::Node, source: &[u8]) -> String {
    let mut cursor = decorator.walk();
    let children: Vec<_> = decorator.named_children(&mut cursor).collect();
    if children.is_empty() {
        return String::new();
    }
    let expr = children[0];
    match expr.kind() {
        "identifier" => node_text(expr, source).to_string(),
        "attribute" => expr
            .child_by_field_name("attribute")
            .map(|a| node_text(a, source).to_string())
            .unwrap_or_default(),
        "call" => {
            let func = match expr.child_by_field_name("function") {
                Some(f) => f,
                None => return String::new(),
            };
            match func.kind() {
                "identifier" => node_text(func, source).to_string(),
                "attribute" => func
                    .child_by_field_name("attribute")
                    .map(|a| node_text(a, source).to_string())
                    .unwrap_or_default(),
                _ => String::new(),
            }
        }
        _ => String::new(),
    }
}

pub fn check_no_overload(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    for node in find_nodes_by_type(source.tree.root_node(), "decorated_definition") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "decorator" {
                let name = extract_decorator_name(child, source.source_bytes);
                if name == "overload" {
                    violations.push(violation(
                        node_line(child),
                        "@overload decorator".to_string(),
                    ));
                }
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_bare_except
// -------------------------------------------------------------------------

pub fn check_no_bare_except(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    for node in find_nodes_by_type(source.tree.root_node(), "except_clause") {
        let mut cursor = node.walk();
        let named: Vec<_> = node.named_children(&mut cursor).collect();
        // Bare except: no named children, or only a block child
        let is_bare = named.is_empty()
            || (named.len() == 1 && named[0].kind() == "block");
        if is_bare {
            violations.push(violation(
                node_line(node),
                "bare except clause".to_string(),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_broad_exceptions
// -------------------------------------------------------------------------

fn except_clause_type_name(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "identifier" => return Some(node_text(child, source).to_string()),
            "attribute" => {
                return child
                    .child_by_field_name("attribute")
                    .map(|a| node_text(a, source).to_string())
            }
            "as_pattern" => {
                let mut inner_cursor = child.walk();
                for inner in child.named_children(&mut inner_cursor) {
                    if inner.kind() == "identifier" {
                        return Some(node_text(inner, source).to_string());
                    }
                    if inner.kind() == "attribute" {
                        return inner
                            .child_by_field_name("attribute")
                            .map(|a| node_text(a, source).to_string());
                    }
                }
            }
            _ => {}
        }
    }
    None
}

pub fn check_no_broad_exceptions(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    for node in find_nodes_by_type(source.tree.root_node(), "except_clause") {
        if let Some(name) = except_clause_type_name(node, source.source_bytes) {
            if name == "Exception" {
                violations.push(violation(
                    node_line(node),
                    "except Exception is too broad".to_string(),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_print
// -------------------------------------------------------------------------

/// True when `print(` appears as a standalone name on the line — not as the
/// tail of a longer identifier such as `fingerprint(` or `sprint(`.
fn has_bare_print_call(line: &str, pattern: &str) -> bool {
    let mut search_from = 0;
    while let Some(offset) = line[search_from..].find(pattern) {
        let start = search_from + offset;
        let preceded_by_name_char = line[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !preceded_by_name_char {
            return true;
        }
        search_from = start + pattern.len();
    }
    false
}

pub fn check_no_print(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    // Pattern constructed at runtime to avoid self-triggering
    let pattern = format!("{}(", "print");
    let mut violations = Vec::new();

    for (line_num, line) in source.lines.iter().enumerate() {
        let line_num = line_num + 1;
        if line.trim_start().starts_with('#') {
            continue;
        }
        if has_bare_print_call(line.as_ref(), &pattern) {
            violations.push(violation(line_num, "print() call".to_string()));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_model_dump
// -------------------------------------------------------------------------

// Reversed to avoid triggering text scanner
const REVERSED_MODEL_DUMP: &[&str] = &[
    ")(pmud_ledom.",
    ")(tcid.",
];

fn model_dump_patterns() -> Vec<String> {
    REVERSED_MODEL_DUMP
        .iter()
        .map(|s| s.chars().rev().collect())
        .collect()
}

pub fn check_no_model_dump(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let patterns = model_dump_patterns();
    let mut violations = Vec::new();

    for (line_num, line) in source.lines.iter().enumerate() {
        let line_num = line_num + 1;
        if line.trim_start().starts_with('#') {
            continue;
        }
        for pattern in &patterns {
            if line.contains(pattern.as_str()) {
                violations.push(violation(
                    line_num,
                    "serialization call sheds type safety".to_string(),
                ));
                break;
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_future_annotations
// -------------------------------------------------------------------------

pub fn check_no_future_annotations(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    // Pattern constructed at runtime
    let pattern = format!("from {} import annotations", "__future__");
    let mut violations = Vec::new();

    for (line_num, line) in source.lines.iter().enumerate() {
        let line_num = line_num + 1;
        if line.contains(&pattern) {
            violations.push(violation(
                line_num,
                "from __future__ import annotations".to_string(),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// init_files_empty
// -------------------------------------------------------------------------

pub fn check_init_files_empty(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let filename = source.file_path.rsplit('/').next().unwrap_or(source.file_path);
    if filename != "__init__.py" {
        return Vec::new();
    }

    // Warning marker constructed at runtime
    let marker = format!("This {} file is EPHEMERAL:", "__init__.py");

    let content = std::str::from_utf8(source.source_bytes).unwrap_or("");
    let trimmed = content.trim();

    if trimmed.is_empty() || trimmed.contains(&marker) {
        return Vec::new();
    }

    vec![violation(1, "non-empty __init__.py".to_string())]
}

// -------------------------------------------------------------------------
// no_nested_functions — any function_definition inside another
// -------------------------------------------------------------------------

/// Detect nested function definitions.
///
/// Walks the entire tree looking for function_definition nodes whose
/// ancestor chain includes another function_definition. Distinguishes
/// closures (inner function references names from outer scope) from
/// plain nested definitions — each gets a different check name so
/// gleipnir_messages.toml can provide targeted guidance.
///
/// Returns violations tagged either "no_nested_functions" or "no_closures".
pub fn check_no_nested_functions(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    let root = source.tree.root_node();
    collect_nested_functions(root, None, source.source_bytes, &mut violations);
    violations
}

fn collect_nested_functions(
    node: tree_sitter::Node,
    enclosing_func: Option<tree_sitter::Node>,
    source: &[u8],
    violations: &mut Vec<Violation>,
) {
    let is_func = node.kind() == "function_definition";

    if is_func {
        if let Some(outer) = enclosing_func {
            let inner_name = node_field(node, "name")
                .map(|n| node_text(n, source))
                .unwrap_or("<anonymous>");

            // Determine if this is a closure (captures names from outer scope)
            let outer_params = collect_param_names(outer, source);
            let outer_locals = collect_local_names(outer, source);
            let mut outer_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for name in &outer_params {
                outer_names.insert(name.as_str());
            }
            for name in &outer_locals {
                outer_names.insert(name.as_str());
            }

            let inner_refs = collect_free_references(node, source);
            let is_closure = inner_refs.iter().any(|name| outer_names.contains(name.as_str()));

            if is_closure {
                violations.push(Violation {
                    line: node_line(node),
                    check_name: "no_closures".to_string(),
                    severity: Severity::Error,
                    message: format!("closure '{inner_name}' captures from enclosing scope"),
                    detail: String::new(),
                    signal: String::new(),
                    direction: String::new(),
                    canary: String::new(),
                });
            } else {
                violations.push(Violation {
                    line: node_line(node),
                    check_name: "no_nested_functions".to_string(),
                    severity: Severity::Error,
                    message: format!("nested function '{inner_name}'"),
                    detail: String::new(),
                    signal: String::new(),
                    direction: String::new(),
                    canary: String::new(),
                });
            }
        }
    }

    let current_func = if is_func { Some(node) } else { enclosing_func };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_nested_functions(child, current_func, source, violations);
    }
}

/// Collect parameter names from a function_definition.
fn collect_param_names(func: tree_sitter::Node, source: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let params = match node_field(func, "parameters") {
        Some(p) => p,
        None => return names,
    };
    let mut cursor = params.walk();
    for child in params.named_children(&mut cursor) {
        match child.kind() {
            "identifier" => names.push(node_text(child, source).to_string()),
            "typed_parameter" | "typed_default_parameter" | "default_parameter" => {
                if let Some(name_node) = child.named_child(0) {
                    if name_node.kind() == "identifier" {
                        names.push(node_text(name_node, source).to_string());
                    }
                }
            }
            _ => {}
        }
    }
    names
}

/// Collect local variable names from assignments in a function body.
fn collect_local_names(func: tree_sitter::Node, source: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let body = match node_field(func, "body") {
        Some(b) => b,
        None => return names,
    };
    for assign in find_nodes_by_type(body, "assignment") {
        if let Some(left) = node_field(assign, "left") {
            if left.kind() == "identifier" {
                names.push(node_text(left, source).to_string());
            }
        }
    }
    names
}

/// Collect identifier references inside a function that aren't its own params/locals.
/// This is approximate — good enough for closure detection.
fn collect_free_references(func: tree_sitter::Node, source: &[u8]) -> Vec<String> {
    let own_params = collect_param_names(func, source);
    let own_locals = collect_local_names(func, source);
    let mut own_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for name in own_params {
        own_names.insert(name);
    }
    for name in own_locals {
        own_names.insert(name);
    }

    let body = match node_field(func, "body") {
        Some(b) => b,
        None => return Vec::new(),
    };

    let mut refs = Vec::new();
    for ident in find_nodes_by_type(body, "identifier") {
        let name = node_text(ident, source);
        // Skip builtins and common names
        if name == "self" || name == "cls" || name == "True" || name == "False" || name == "None" {
            continue;
        }
        if !own_names.contains(name) {
            refs.push(name.to_string());
        }
    }
    refs
}

// -------------------------------------------------------------------------
// no_general_lambda — lambda expressions outside default_factory=
// -------------------------------------------------------------------------

/// Check if a lambda node is the value of a `default_factory=` keyword argument.
fn is_default_factory_lambda(node: tree_sitter::Node, source: &[u8]) -> bool {
    let parent = match node.parent() {
        Some(p) => p,
        None => return false,
    };
    if parent.kind() != "keyword_argument" {
        return false;
    }
    match node_field(parent, "name") {
        Some(name_node) => node_text(name_node, source) == "default_factory",
        None => false,
    }
}

/// Detect lambda expressions that are NOT in `default_factory=` position.
///
/// General-purpose lambdas are anonymous closures — same problem as
/// no_closures but harder to spot because there is no name to grep for.
pub fn check_no_general_lambda(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    find_nodes_by_type(source.tree.root_node(), "lambda")
        .iter()
        .filter(|node| !is_default_factory_lambda(**node, source.source_bytes))
        .map(|node| Violation {
            line: node_line(*node),
            check_name: "no_general_lambda".to_string(),
            severity: Severity::Error,
            message: "lambda expression — extract to a named function".to_string(),
            detail: String::new(),
            signal: String::new(),
            direction: String::new(),
            canary: String::new(),
        })
        .collect()
}

// -------------------------------------------------------------------------
// no_default_factory — default_factory=lambda in Pydantic fields
// -------------------------------------------------------------------------

/// Detect `default_factory=lambda` in Pydantic field definitions.
///
/// Generated structure files (structure/gen/) are exempt via the matrix —
/// this check is not wired into V2_GEN_CHECKS.
pub fn check_no_default_factory(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    find_nodes_by_type(source.tree.root_node(), "lambda")
        .iter()
        .filter(|node| is_default_factory_lambda(**node, source.source_bytes))
        .map(|node| Violation {
            line: node_line(*node),
            check_name: "no_default_factory".to_string(),
            severity: Severity::Error,
            message: "default_factory=lambda — use a named function or class default".to_string(),
            detail: String::new(),
            signal: String::new(),
            direction: String::new(),
            canary: String::new(),
        })
        .collect()
}

// -------------------------------------------------------------------------
// no_partial — functools.partial creates anonymous callables
// -------------------------------------------------------------------------

/// Detect calls to functools.partial or bare partial().
///
/// partial() is a lambda factory — it creates anonymous callables that
/// bypass the lambda ban while producing the same ungreppable, untestable
/// closures. If you need to bind arguments, write a named function.
pub fn check_no_partial(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    for node in find_nodes_by_type(source.tree.root_node(), "call") {
        let func = match node.child_by_field_name("function") {
            Some(f) => f,
            None => continue,
        };
        let is_partial = match func.kind() {
            "identifier" => node_text(func, source.source_bytes) == "partial",
            "attribute" => func
                .child_by_field_name("attribute")
                .is_some_and(|a| node_text(a, source.source_bytes) == "partial"),
            _ => false,
        };
        if is_partial {
            violations.push(violation(
                node_line(node),
                "functools.partial() — write a named function instead".to_string(),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_re_sub — re.sub with callbacks encourages lambda/partial
// -------------------------------------------------------------------------

/// Detect calls to re.sub().
///
/// re.sub with a callable replacement encourages lambda and partial patterns.
/// Use re.finditer() with a loop instead — explicit, readable, no callbacks.
pub fn check_no_re_sub(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    for node in find_nodes_by_type(source.tree.root_node(), "call") {
        let func = match node.child_by_field_name("function") {
            Some(f) => f,
            None => continue,
        };
        if func.kind() == "attribute" {
            let object = func.child_by_field_name("object");
            let attr = func.child_by_field_name("attribute");
            if let (Some(obj), Some(attr)) = (object, attr) {
                if node_text(obj, source.source_bytes) == "re"
                    && node_text(attr, source.source_bytes) == "sub"
                {
                    violations.push(violation(
                        node_line(node),
                        "re.sub() — use re.finditer() with a loop instead".to_string(),
                    ));
                }
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_recursion — function calls itself by name
// -------------------------------------------------------------------------

pub fn check_no_recursion(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    let root = source.tree.root_node();
    collect_recursive_calls(root, None, source.source_bytes, &mut violations);
    violations
}

fn collect_recursive_calls(
    node: tree_sitter::Node,
    enclosing_func_name: Option<&str>,
    source: &[u8],
    violations: &mut Vec<Violation>,
) {
    let is_func = node.kind() == "function_definition";
    let func_name = if is_func {
        node_field(node, "name").map(|n| node_text(n, source))
    } else {
        None
    };

    if let Some(name) = func_name {
        find_self_calls(node, name, source, violations);
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect_recursive_calls(child, Some(name), source, violations);
        }
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_recursive_calls(child, enclosing_func_name, source, violations);
    }
}

fn find_self_calls(
    func_node: tree_sitter::Node,
    func_name: &str,
    source: &[u8],
    violations: &mut Vec<Violation>,
) {
    let body = match node_field(func_node, "body") {
        Some(b) => b,
        None => return,
    };
    for call in find_nodes_by_type(body, "call") {
        let callee = match node_field(call, "function") {
            Some(f) => f,
            None => continue,
        };
        if callee.kind() == "identifier" && node_text(callee, source) == func_name {
            violations.push(violation(
                node_line(call),
                format!("recursive call to '{func_name}'"),
            ));
        }
    }
}

// -------------------------------------------------------------------------
// no_dunder_all
// -------------------------------------------------------------------------

pub fn check_no_dunder_all(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    let root = source.tree.root_node();
    let mut cursor = root.walk();

    for stmt in root.named_children(&mut cursor) {
        let assign = if stmt.kind() == "expression_statement" {
            let mut inner_cursor = stmt.walk();
            let first = stmt.named_children(&mut inner_cursor)
                .next()
                .filter(|c| c.kind() == "assignment");
            first
        } else if stmt.kind() == "assignment" {
            Some(stmt)
        } else {
            None
        };

        let assign = match assign {
            Some(a) => a,
            None => continue,
        };

        if let Some(left) = node_field(assign, "left") {
            if left.kind() == "identifier" && node_text(left, source.source_bytes) == "__all__" {
                violations.push(violation(
                    node_line(assign),
                    "__all__ declaration".to_string(),
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

    fn parse_init(code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source("/test/__init__.py", source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside, &crate::STATISTICS)
    }

    // -- no_cast --

    #[test]
    fn cast_call_caught() {
        let parsed = parse("from typing import cast\nx = cast(int, value)\n");
        let violations = check_no_cast(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn no_cast_clean() {
        let parsed = parse("x = int(value)\n");
        let violations = check_no_cast(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_overload --

    #[test]
    fn overload_caught() {
        let parsed = parse("from typing import overload\n@overload\ndef foo(x: int) -> int: ...\n");
        let violations = check_no_overload(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- no_bare_except --

    #[test]
    fn bare_except_caught() {
        let parsed = parse("try:\n    pass\nexcept:\n    pass\n");
        let violations = check_no_bare_except(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn typed_except_ok() {
        let parsed = parse("try:\n    pass\nexcept ValueError:\n    pass\n");
        let violations = check_no_bare_except(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_broad_exceptions --

    #[test]
    fn broad_exception_caught() {
        let parsed = parse("try:\n    pass\nexcept Exception:\n    pass\n");
        let violations = check_no_broad_exceptions(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn specific_exception_ok() {
        let parsed = parse("try:\n    pass\nexcept ValueError:\n    pass\n");
        let violations = check_no_broad_exceptions(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_print --

    #[test]
    fn print_call_caught() {
        let code = "print(\"hello\")\n";
        let parsed = parse(code);
        let violations = check_no_print(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn no_print_clean() {
        let parsed = parse("x = 42\n");
        let violations = check_no_print(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn fingerprint_call_not_flagged() {
        // Specimen: `print(` as the tail of a longer identifier is not the builtin.
        let parsed = parse("digest = fingerprint(data)\nshort = sprint(value)\n");
        let violations = check_no_print(&parsed, &default_config());
        assert!(violations.is_empty(), "{:?}", violations.iter().map(|v| v.line).collect::<Vec<_>>());
    }

    #[test]
    fn indented_and_nested_print_still_caught() {
        let code = "def show(x):\n    print(x)\n    return len(print(x) or [])\n";
        let parsed = parse(code);
        let violations = check_no_print(&parsed, &default_config());
        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn fingerprint_then_print_same_line_caught() {
        let parsed = parse("print(fingerprint(data))\n");
        let violations = check_no_print(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- no_future_annotations --

    #[test]
    fn future_annotations_caught() {
        let code = "from __future__ import annotations\n";
        let parsed = parse(code);
        let violations = check_no_future_annotations(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- init_files_empty --

    #[test]
    fn nonempty_init_caught() {
        let parsed = parse_init("from foo import bar\n");
        let violations = check_init_files_empty(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn empty_init_ok() {
        let parsed = parse_init("");
        let violations = check_init_files_empty(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_dunder_all --

    #[test]
    fn dunder_all_caught() {
        let parsed = parse("__all__ = ['foo', 'bar']\n");
        let violations = check_no_dunder_all(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn no_dunder_all_clean() {
        let parsed = parse("foo = 42\n");
        let violations = check_no_dunder_all(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_model_dump --

    #[test]
    fn model_dump_caught() {
        let code = "result = obj.model_dump()\n";
        let parsed = parse(code);
        let violations = check_no_model_dump(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- no_nested_functions --

    #[test]
    fn nested_function_caught() {
        let code = "def outer():\n    def inner():\n        return 1\n    return inner()\n";
        let parsed = parse(code);
        let violations = check_no_nested_functions(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].check_name, "no_nested_functions");
        assert!(violations[0].message.contains("inner"));
    }

    #[test]
    fn closure_caught_with_correct_check_name() {
        let code = "def outer(data):\n    x = 10\n    def inner():\n        return x + data\n    return inner()\n";
        let parsed = parse(code);
        let violations = check_no_nested_functions(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].check_name, "no_closures");
        assert!(violations[0].message.contains("closure"));
    }

    #[test]
    fn flat_functions_ok() {
        let code = "def foo():\n    return 1\ndef bar():\n    return 2\n";
        let parsed = parse(code);
        let violations = check_no_nested_functions(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn doubly_nested_both_caught() {
        let code = "def a():\n    def b():\n        def c():\n            pass\n        pass\n    pass\n";
        let parsed = parse(code);
        let violations = check_no_nested_functions(&parsed, &default_config());
        assert_eq!(violations.len(), 2); // b and c both flagged
    }

    // -- no_general_lambda --

    #[test]
    fn general_lambda_in_function_call_caught() {
        let code = "import re\ndef interpolate(template: str, values: dict) -> str:\n    return re.sub(r'pattern', lambda m: values.get(m.group(1)), template)\n";
        let parsed = parse(code);
        let violations = check_no_general_lambda(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].check_name, "no_general_lambda");
    }

    #[test]
    fn general_lambda_assigned_to_variable_caught() {
        let code = "double = lambda x: x * 2\n";
        let parsed = parse(code);
        let violations = check_no_general_lambda(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn no_general_lambda_clean_code_ok() {
        let code = "def add(a: int, b: int) -> int:\n    return a + b\n";
        let parsed = parse(code);
        let violations = check_no_general_lambda(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn multiple_general_lambdas_all_caught() {
        let code = "items.sort(key=lambda x: x.name)\nresult = map(lambda y: y + 1, numbers)\n";
        let parsed = parse(code);
        let violations = check_no_general_lambda(&parsed, &default_config());
        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn default_factory_lambda_not_caught_by_general() {
        let code = "from pydantic import Field\nclass Model:\n    items: list[str] = Field(default_factory=lambda: [])\n";
        let parsed = parse(code);
        let violations = check_no_general_lambda(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_default_factory --

    #[test]
    fn default_factory_lambda_caught() {
        let code = "from pydantic import Field\nclass Model:\n    items: list[str] = Field(default_factory=lambda: [])\n";
        let parsed = parse(code);
        let violations = check_no_default_factory(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].check_name, "no_default_factory");
    }

    #[test]
    fn non_default_factory_lambda_not_caught_by_default_factory() {
        let code = "items.sort(key=lambda x: x.name)\n";
        let parsed = parse(code);
        let violations = check_no_default_factory(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn default_factory_with_complex_body_caught() {
        let code = "from pydantic import Field\nclass Model:\n    data: dict[str, int] = Field(default_factory=lambda: build_defaults())\n";
        let parsed = parse(code);
        let violations = check_no_default_factory(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- no_recursion --

    #[test]
    fn recursive_call_caught() {
        let code = "def factorial(n):\n    if n <= 1:\n        return 1\n    return n * factorial(n - 1)\n";
        let parsed = parse(code);
        let violations = check_no_recursion(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("factorial"));
    }

    #[test]
    fn non_recursive_call_ok() {
        let code = "def foo():\n    return bar()\ndef bar():\n    return 1\n";
        let parsed = parse(code);
        let violations = check_no_recursion(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn method_call_same_name_not_flagged() {
        // obj.process() inside def process() — the callee is an attribute, not an identifier
        let code = "def process(data):\n    return data.process()\n";
        let parsed = parse(code);
        let violations = check_no_recursion(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_partial --

    #[test]
    fn partial_call_caught() {
        let code = "from functools import partial\ndef resolve(values, match):\n    return values[match]\ncallback = partial(resolve, my_values)\n";
        let parsed = parse(code);
        let violations = check_no_partial(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn functools_partial_attribute_caught() {
        let code = "import functools\ncallback = functools.partial(resolve, my_values)\n";
        let parsed = parse(code);
        let violations = check_no_partial(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn no_partial_clean_code_ok() {
        let code = "def resolve(values: dict, match: str) -> str:\n    return values[match]\n";
        let parsed = parse(code);
        let violations = check_no_partial(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_re_sub --

    #[test]
    fn re_sub_caught() {
        let code = "import re\nresult = re.sub(r'pattern', 'replacement', text)\n";
        let parsed = parse(code);
        let violations = check_no_re_sub(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn re_sub_with_callback_caught() {
        let code = "import re\nresult = re.sub(r'pattern', callback_fn, text)\n";
        let parsed = parse(code);
        let violations = check_no_re_sub(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn re_finditer_ok() {
        let code = "import re\nfor match in re.finditer(r'pattern', text):\n    pass\n";
        let parsed = parse(code);
        let violations = check_no_re_sub(&parsed, &default_config());
        assert!(violations.is_empty());
    }

}
