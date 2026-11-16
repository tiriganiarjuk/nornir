//! Style and complexity checks for Rust source files.
//!
//! Checks: function_length, param_count, nesting_depth, no_underscore_prefix,
//! no_single_letter_names, no_numbered_suffixes, short_param_names.

use crate::numbered_names;
use crate::parsing::{find_nodes_by_type, node_line, node_text};
use crate::structures::{CheckConfig, ParsedSource, Severity, Violation};

use super::prohibited::in_test_context;

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

/// Count non-blank, non-comment lines in a function body.
fn count_code_lines_rust(func_node: tree_sitter::Node, lines: &[&str]) -> usize {
    let start_row = func_node.start_position().row;
    let end_row = func_node.end_position().row;
    let mut count = 0;

    for row in (start_row + 1)..=end_row.min(lines.len().saturating_sub(1)) {
        let stripped = lines[row].trim();
        if stripped.is_empty() || stripped.starts_with("//") || stripped == "{" || stripped == "}" {
            continue;
        }
        count += 1;
    }
    count
}

/// Get function name from a function_item node.
fn func_name<'a>(node: tree_sitter::Node<'a>, source: &'a [u8]) -> &'a str {
    node.child_by_field_name("name")
        .map(|n| node_text(n, source))
        .unwrap_or("")
}

pub fn check_function_length(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_item") {
        let name = func_name(func_node, source.source_bytes);

        // Skip test functions
        if in_test_context(func_node, source) {
            continue;
        }

        let code_lines = count_code_lines_rust(func_node, &source.lines);
        if code_lines > config.max_function_lines {
            violations.push(violation(
                node_line(func_node),
                format!("{name}() too long"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// param_count
// -------------------------------------------------------------------------

/// Extract parameter names from a Rust function's parameters node.
fn rust_param_names<'a>(
    func_node: tree_sitter::Node<'a>,
    source: &'a [u8],
) -> Vec<(usize, &'a str)> {
    let params_node = match func_node.child_by_field_name("parameters") {
        Some(n) => n,
        None => return Vec::new(),
    };

    let mut results = Vec::new();
    let mut cursor = params_node.walk();
    for param in params_node.named_children(&mut cursor) {
        match param.kind() {
            "parameter" => {
                if let Some(pattern) = param.child_by_field_name("pattern") {
                    let name = node_text(pattern, source);
                    // Skip self parameters
                    if name != "self" && name != "_" {
                        results.push((node_line(param), name));
                    }
                }
            }
            "self_parameter" => {} // Skip self
            _ => {}
        }
    }
    results
}

pub fn check_param_count(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_item") {
        let name = func_name(func_node, source.source_bytes);

        if in_test_context(func_node, source) {
            continue;
        }

        let params = rust_param_names(func_node, source.source_bytes);
        if params.len() > config.max_function_params {
            violations.push(violation(
                node_line(func_node),
                format!("{name}() has too many parameters"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// nesting_depth
// -------------------------------------------------------------------------

const RUST_NESTING_TYPES: &[&str] = &[
    "if_expression",
    "for_expression",
    "while_expression",
    "loop_expression",
    "match_expression",
];

fn compute_max_nesting_rust(node: tree_sitter::Node, current: usize) -> usize {
    let mut best = current;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if RUST_NESTING_TYPES.contains(&child.kind()) {
            best = best.max(compute_max_nesting_rust(child, current + 1));
        } else {
            best = best.max(compute_max_nesting_rust(child, current));
        }
    }
    best
}

pub fn check_nesting_depth(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_item") {
        let name = func_name(func_node, source.source_bytes);

        if in_test_context(func_node, source) {
            continue;
        }

        let depth = compute_max_nesting_rust(func_node, 0);
        if depth > config.max_nesting_depth {
            violations.push(violation(
                node_line(func_node),
                format!("{name}() nested too deeply"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_underscore_prefix
// -------------------------------------------------------------------------

fn has_underscore_prefix(name: &str) -> bool {
    name.starts_with('_') && name != "_"
}

/// Check if an underscore-prefixed let binding is actually used in its enclosing block.
/// Walks up to the nearest block and counts identifier matches — if > 1
/// (the binding itself plus at least one use), the underscore is a lie.
fn underscore_let_is_used(let_node: tree_sitter::Node, var_name: &str, source: &[u8]) -> bool {
    let block = match find_enclosing_block(let_node) {
        Some(b) => b,
        None => return false,
    };
    let mut count = 0;
    for ident in find_nodes_by_type(block, "identifier") {
        if node_text(ident, source) == var_name {
            count += 1;
            if count > 1 {
                return true;
            }
        }
    }
    false
}

fn find_enclosing_block(node: tree_sitter::Node) -> Option<tree_sitter::Node> {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if ancestor.kind() == "block" {
            return Some(ancestor);
        }
        current = ancestor.parent();
    }
    None
}

/// Check if a name appears as an identifier in a subtree.
fn name_used_in_subtree(root: tree_sitter::Node, name: &str, source: &[u8]) -> bool {
    for ident in find_nodes_by_type(root, "identifier") {
        if node_text(ident, source) == name {
            return true;
        }
    }
    false
}

fn find_underscore_params<'a>(
    func_node: tree_sitter::Node<'a>,
    body: Option<tree_sitter::Node<'a>>,
    func_name: &str,
    source: &[u8],
) -> Vec<Violation> {
    let body_node = match body {
        Some(b) => b,
        None => return vec![],
    };
    rust_param_names(func_node, source)
        .into_iter()
        .filter(|(_, pname)| has_underscore_prefix(pname))
        .filter(|(_, pname)| name_used_in_subtree(body_node, pname, source))
        .map(|(line, pname)| {
            violation(line, format!("parameter '{pname}' uses underscore prefix in {func_name}()"))
        })
        .collect()
}

pub fn check_no_underscore_prefix(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_item") {
        let name = func_name(func_node, source.source_bytes);

        if in_test_context(func_node, source) {
            continue;
        }

        // Function names: always flagged (functions aren't "unused" in the compiler sense)
        if has_underscore_prefix(name) {
            violations.push(violation(
                node_line(func_node),
                format!("function '{name}' uses underscore prefix"),
            ));
        }

        let body = func_node.child_by_field_name("body");
        violations.extend(find_underscore_params(func_node, body, name, source.source_bytes));
    }

    // Let bindings: only flag if used after the binding (count > 1 in enclosing block)
    for let_node in find_nodes_by_type(source.tree.root_node(), "let_declaration") {
        if in_test_context(let_node, source) {
            continue;
        }
        let pattern = match let_node.child_by_field_name("pattern") {
            Some(p) if p.kind() == "identifier" => p,
            _ => continue,
        };
        let var_name = node_text(pattern, source.source_bytes);
        if !has_underscore_prefix(var_name) {
            continue;
        }
        if underscore_let_is_used(let_node, var_name, source.source_bytes) {
            violations.push(violation(
                node_line(let_node),
                format!("variable '{var_name}' uses underscore prefix"),
            ));
        }
    }

    violations
}

// -------------------------------------------------------------------------
// Naming quality helpers
// -------------------------------------------------------------------------

const SINGLE_LETTER_ALLOWLIST: &[&str] = &["i", "j", "k", "_", "f", "m"];

const SHORT_PARAM_ALLOWLIST: &[&str] = &[
    "id", "db", "ok", "io", "fn", "tx", "rx", "ui", "pk", "op",
    "fd", "ip", "cx", "f", "map", "py", "m", "sig", "key",
];

fn is_bad_numbered_name(name: &str) -> bool {
    numbered_names::rust_numbered().is_match(name) && !numbered_names::rust_allowed().is_match(name)
}

/// Collect let-binding names inside a function body.
fn collect_let_names<'a>(
    func_node: tree_sitter::Node<'a>,
    source: &'a [u8],
) -> Vec<(usize, &'a str)> {
    let body = match func_node.child_by_field_name("body") {
        Some(b) => b,
        None => return Vec::new(),
    };
    let mut results = Vec::new();
    for node in find_nodes_by_type(body, "let_declaration") {
        if let Some(pattern) = node.child_by_field_name("pattern") {
            if pattern.kind() == "identifier" {
                let name = node_text(pattern, source);
                if name != "_" {
                    results.push((node_line(node), name));
                }
            }
        }
    }
    results
}

// -------------------------------------------------------------------------
// no_single_letter_names
// -------------------------------------------------------------------------

pub fn check_no_single_letter_names(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_item") {
        let name = func_name(func_node, source.source_bytes);

        if in_test_context(func_node, source) {
            continue;
        }

        // Check parameters
        for (line, pname) in rust_param_names(func_node, source.source_bytes) {
            if pname.len() == 1 && !SINGLE_LETTER_ALLOWLIST.contains(&pname) {
                violations.push(violation(
                    line,
                    format!("single-letter parameter '{pname}' in {name}()"),
                ));
            }
        }

        // Check let bindings
        for (line, var_name) in collect_let_names(func_node, source.source_bytes) {
            if var_name.len() == 1 && !SINGLE_LETTER_ALLOWLIST.contains(&var_name) {
                violations.push(violation(
                    line,
                    format!("single-letter variable '{var_name}' in {name}()"),
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

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_item") {
        let name = func_name(func_node, source.source_bytes);

        if in_test_context(func_node, source) {
            continue;
        }

        for (line, pname) in rust_param_names(func_node, source.source_bytes) {
            if is_bad_numbered_name(pname) {
                violations.push(violation(
                    line,
                    format!("numbered parameter '{pname}' in {name}()"),
                ));
            }
        }

        for (line, var_name) in collect_let_names(func_node, source.source_bytes) {
            if is_bad_numbered_name(var_name) {
                violations.push(violation(
                    line,
                    format!("numbered variable '{var_name}' in {name}()"),
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

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_item") {
        let name = func_name(func_node, source.source_bytes);

        if in_test_context(func_node, source) {
            continue;
        }

        for (line, pname) in rust_param_names(func_node, source.source_bytes) {
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
    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::build_parsed_source_rust;
    use crate::structures::FileKind;

    fn parse(code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source_rust("/test/file.rs", source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside, &crate::STATISTICS)
    }

    // -- function_length --

    #[test]
    fn short_function_ok() {
        let parsed = parse("fn foo() -> i32 { 1 }");
        let violations = check_function_length(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn long_function_caught() {
        let mut code = String::from("fn foo() {\n");
        for i in 0..55 {
            code.push_str(&format!("    let x{i} = {i};\n"));
        }
        code.push_str("}\n");
        let parsed = parse(&code);
        let violations = check_function_length(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("foo"));
    }

    #[test]
    fn test_function_length_skipped() {
        let mut code = String::from("#[test]\nfn test_big() {\n");
        for i in 0..55 {
            code.push_str(&format!("    let x{i} = {i};\n"));
        }
        code.push_str("}\n");
        let parsed = parse(&code);
        let violations = check_function_length(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- param_count --

    #[test]
    fn few_params_ok() {
        let parsed = parse("fn foo(a: i32, b: i32, c: i32) {}");
        let violations = check_param_count(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn too_many_params_caught() {
        let parsed = parse("fn foo(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, g: i32) {}");
        let violations = check_param_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("foo"));
    }

    #[test]
    fn self_param_excluded() {
        let parsed = parse(
            r#"
            impl Foo {
                fn bar(&self, a: i32, b: i32, c: i32, d: i32, e: i32) {}
            }
            "#
        );
        let violations = check_param_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 5 params after excluding self
    }

    // -- nesting_depth --

    #[test]
    fn shallow_nesting_ok() {
        let parsed = parse("fn foo() { if true { return; } }");
        let violations = check_nesting_depth(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn deep_nesting_caught() {
        let code = r#"
fn foo() {
    if true {
        for x in 0..10 {
            if x > 0 {
                while true {
                    if x > 5 {
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
        assert!(violations[0].message.contains("foo"));
    }

    #[test]
    fn match_inside_for_ok() {
        // for(1) -> match(2) -> if(3) = depth 3. Arms are alternatives, not depth.
        let code = r#"
fn dispatch(items: &[i32]) {
    for item in items {
        match item {
            0 => {
                if true {
                    return;
                }
            }
            1 => {
                for sub in items {
                    if *sub > 0 {
                        continue;
                    }
                }
            }
            _ => {}
        }
    }
}
"#;
        let parsed = parse(code);
        let violations = check_nesting_depth(&parsed, &default_config());
        assert!(violations.is_empty(), "match arms should not count as nesting depth");
    }

    #[test]
    fn nested_match_expressions_caught() {
        // match(1) -> match(2) -> match(3) -> if(4) -> if(5) = depth 5. Still caught.
        let code = r#"
fn deeply_nested(val: Option<Option<Option<i32>>>) {
    match val {
        Some(inner1) => {
            match inner1 {
                Some(inner2) => {
                    match inner2 {
                        Some(x) => {
                            if x > 0 {
                                if x > 10 {
                                    return;
                                }
                            }
                        }
                        None => {}
                    }
                }
                None => {}
            }
        }
        None => {}
    }
}
"#;
        let parsed = parse(code);
        let violations = check_nesting_depth(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "nested match expressions should still be caught");
        assert!(violations[0].message.contains("deeply_nested"));
    }

    // -- no_underscore_prefix --

    #[test]
    fn underscore_function_always_caught() {
        let parsed = parse("fn _helper() {}");
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("_helper"));
    }

    #[test]
    fn single_underscore_ok() {
        let parsed = parse("fn foo() { let _ = 1; }");
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn genuinely_unused_param_ok() {
        // _config is never used in the body — underscore is correct
        let parsed = parse("fn foo(_unused: i32) { let x = 1; }");
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn used_underscore_param_caught() {
        // _used appears in the body — underscore is a lie
        let code = "fn foo(_used: i32) { let x = _used + 1; }";
        let parsed = parse(code);
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("_used"));
    }

    #[test]
    fn genuinely_unused_let_ok() {
        // _cache is bound but never read — underscore is correct
        let parsed = parse("fn foo() { let _cache = vec![]; }");
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn used_underscore_let_caught() {
        // _cache is bound AND used — underscore is a lie
        let code = "fn foo() { let _cache = vec![1]; let x = _cache.len(); }";
        let parsed = parse(code);
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("_cache"));
    }

    // -- no_single_letter_names --

    #[test]
    fn single_letter_param_caught() {
        let parsed = parse("fn foo(x: i32) {}");
        let violations = check_no_single_letter_names(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("'x'"));
    }

    #[test]
    fn loop_index_ok() {
        let parsed = parse("fn foo() { for i in 0..10 {} }");
        let violations = check_no_single_letter_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn single_letter_local_caught() {
        let parsed = parse("fn foo() { let x = 1; }");
        let violations = check_no_single_letter_names(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("variable"));
    }

    // -- no_numbered_suffixes --

    #[test]
    fn numbered_param_caught() {
        let parsed = parse("fn foo(result1: i32) {}");
        let violations = check_no_numbered_suffixes(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("result1"));
    }

    #[test]
    fn allowed_numbered_ok() {
        let parsed = parse("fn foo(sha256: &str) {}");
        let violations = check_no_numbered_suffixes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn u8_type_ok() {
        // u8, i32, f64 — allowed number suffixes
        let parsed = parse("fn foo(u8: u8) {}");
        let violations = check_no_numbered_suffixes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn numbered_local_caught() {
        let parsed = parse("fn foo() { let data1 = 1; }");
        let violations = check_no_numbered_suffixes(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("data1"));
    }

    // -- short_param_names --

    #[test]
    fn short_param_caught() {
        let parsed = parse("fn foo(ab: i32) {}");
        let violations = check_short_param_names(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("'ab'"));
    }

    #[test]
    fn allowlisted_short_ok() {
        let parsed = parse("fn foo(id: i32, db: &str, tx: Sender) {}");
        let violations = check_short_param_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn long_param_ok() {
        let parsed = parse("fn foo(user_name: &str) {}");
        let violations = check_short_param_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn underscore_prefix_param_skipped() {
        let parsed = parse("fn foo(_x: i32) {}");
        let violations = check_short_param_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }
}
