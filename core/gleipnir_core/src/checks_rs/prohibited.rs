//! Prohibited pattern checks for Rust source files.
//!
//! Checks: no_unwrap, no_println, no_clone_spam, no_string_abuse, no_pub_overuse

use crate::parsing::{find_nodes_by_type, node_line, node_text};
use crate::structures::{CheckConfig, ParsedSource, Severity, Violation};

fn violation(line: usize, message: impl Into<String>) -> Violation {
    Violation {
        line,
        check_name: String::new(),
        severity: Severity::Error,
        message: message.into(),
        detail: String::new(),
        signal: String::new(),
        direction: String::new(),
        canary: String::new(),
    }
}

// -------------------------------------------------------------------------
// Shared: test context detection
// -------------------------------------------------------------------------

/// True for a Rust integration-test file — one under a crate's `tests/`
/// directory, where every function is test code whether or not it carries
/// `#[test]`. The path has a `tests` directory, and no `src` directory below the
/// last one: a crate kept under `tests/` as a fixture has its own `src/`, and
/// that is production code.
pub(crate) fn path_is_test_file(file_path: &str) -> bool {
    let Some((_, below_tests)) = file_path.rsplit_once("/tests/") else {
        return false;
    };
    let directories_below = below_tests.rsplit_once('/').map_or("", |(directories, _file_name)| directories);
    !directories_below.split('/').any(|directory| directory == "src")
}

// -------------------------------------------------------------------------
// Shared: macro names
// -------------------------------------------------------------------------

/// The name a macro is invoked by: the last `::` segment of its path, so
/// `std::panic` and `panic` are the same macro.
fn macro_base_name(macro_path: &str) -> &str {
    macro_path.rsplit("::").next().unwrap_or(macro_path).trim()
}

/// Check if a node is test code: the file is an integration-test file, or the
/// node is inside a test function or a #[cfg(test)] module.
///
/// In tree-sitter-rust, attributes are previous siblings, not children:
///   attribute_item("#[test]")  <- sibling
///   function_item("fn ...")    <- the function
pub(crate) fn in_test_context(node: tree_sitter::Node, parsed: &ParsedSource) -> bool {
    if path_is_test_file(parsed.file_path) {
        return true;
    }
    let source = parsed.source_bytes;

    // Check the node itself first (when node IS a function_item/mod_item)
    match node.kind() {
        "function_item" => {
            if has_preceding_attribute(node, source, "test") {
                return true;
            }
        }
        "mod_item" => {
            if has_preceding_attribute(node, source, "cfg(test)") {
                return true;
            }
        }
        _ => {}
    }

    // Walk up ancestors
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "function_item" => {
                if has_preceding_attribute(parent, source, "test") {
                    return true;
                }
            }
            "mod_item" => {
                if has_preceding_attribute(parent, source, "cfg(test)") {
                    return true;
                }
            }
            _ => {}
        }
        current = parent.parent();
    }
    false
}

/// Check if the previous sibling(s) of a node are attribute_items containing target text.
pub(crate) fn has_preceding_attribute(node: tree_sitter::Node, source: &[u8], target: &str) -> bool {
    let mut sibling = node.prev_sibling();
    while let Some(sib) = sibling {
        if sib.kind() == "attribute_item" {
            let text = node_text(sib, source);
            if text.contains(target) {
                return true;
            }
        } else {
            break;
        }
        sibling = sib.prev_sibling();
    }
    false
}

// -------------------------------------------------------------------------
// no_unwrap
// -------------------------------------------------------------------------

/// Check if a closure node is passed to an initialization method (get_or_init, get_or_try_init).
fn is_init_closure(closure: tree_sitter::Node, source: &[u8]) -> bool {
    let args = match closure.parent() {
        Some(node) if node.kind() == "arguments" => node,
        _ => return false,
    };
    let call_expr = match args.parent() {
        Some(node) => node,
        None => return false,
    };
    let func = match call_expr.child_by_field_name("function") {
        Some(node) if node.kind() == "field_expression" => node,
        _ => return false,
    };
    let field = match func.child_by_field_name("field") {
        Some(node) => node,
        None => return false,
    };
    let method = node_text(field, source);
    method == "get_or_init" || method == "get_or_try_init"
}

/// Check if a node is inside a static or const initializer.
///
/// `static FOO: LazyLock<T> = LazyLock::new(|| ...unwrap()...);`
///
/// Unwrap/expect in static initializers runs exactly once at first access
/// (LazyLock) or at compile time (const). These are initialization panics
/// on constant data, not runtime panics on variable data.
fn in_initialization_context(node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        match ancestor.kind() {
            "static_item" | "const_item" => return true,
            "function_item" => return false,
            "closure_expression" if is_init_closure(ancestor, source) => return true,
            _ => {}
        }
        current = ancestor.parent();
    }
    false
}

/// Macros that panic when reached.
const PANICKING_MACROS: &[&str] = &[
    "panic",
    "unreachable",
    "todo",
    "unimplemented",
];

/// Detect panic!(), unreachable!(), todo!() and unimplemented!() invocations,
/// with the same exemptions as .unwrap() and .expect().
fn find_panicking_macros(source: &ParsedSource) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "macro_invocation") {
        let macro_node = match node.child_by_field_name("macro") {
            Some(m) => m,
            None => continue,
        };

        let macro_name = macro_base_name(node_text(macro_node, source.source_bytes));
        if !PANICKING_MACROS.contains(&macro_name) {
            continue;
        }
        if in_test_context(node, source) {
            continue;
        }
        if in_initialization_context(node, source.source_bytes) {
            continue;
        }

        violations.push(violation(
            node_line(node),
            format!("{}!() panics when reached — return an error instead", macro_name),
        ));
    }

    violations
}

/// Detect .unwrap() and .expect() calls and panicking macros in Rust source.
///
/// Skips:
/// - Test functions (#[test] or #[cfg(test)] modules)
/// - Static, const and get_or_init initializers
/// - unwrap_or, unwrap_or_default, unwrap_or_else (safe alternatives)
pub fn check_no_unwrap(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = find_panicking_macros(source);

    for node in find_nodes_by_type(source.tree.root_node(), "call_expression") {
        let function_node = match node.child_by_field_name("function") {
            Some(f) => f,
            None => continue,
        };

        if function_node.kind() != "field_expression" {
            continue;
        }

        let field = match function_node.child_by_field_name("field") {
            Some(f) => f,
            None => continue,
        };

        let method_name = node_text(field, source.source_bytes);

        if method_name == "unwrap" || method_name == "expect" {
            if in_test_context(node, source) {
                continue;
            }
            if in_initialization_context(node, source.source_bytes) {
                continue;
            }

            violations.push(violation(
                node_line(node),
                format!(".{}() panics on failure — propagate the error with ? instead", method_name),
            ));
        }
    }

    violations
}

// -------------------------------------------------------------------------
// no_println
// -------------------------------------------------------------------------

/// Debug/print macro names that should not appear in production code.
const FORBIDDEN_MACROS: &[&str] = &[
    "println",
    "print",
    "dbg",
];

/// Check if a node is inside fn main() (binary entry point uses stdout).
fn in_main_function(node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if ancestor.kind() == "function_item" {
            let name = ancestor
                .child_by_field_name("name")
                .map(|n| node_text(n, source))
                .unwrap_or("");
            return name == "main";
        }
        current = ancestor.parent();
    }
    false
}

/// Check if a node is inside a function whose name indicates stdout output.
fn in_output_function(node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if ancestor.kind() == "function_item" {
            let name = ancestor
                .child_by_field_name("name")
                .map(|n| node_text(n, source))
                .unwrap_or("");
            return name.starts_with("print_")
                || name.starts_with("emit_")
                || name.starts_with("display_")
                || name == "print_help"
                || name == "print_usage";
        }
        current = ancestor.parent();
    }
    false
}

/// Nornir crate tier derived from file path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RustCrateTier {
    Core,
    Capability,
    Binary,
    Gate,
    Unknown,
}

fn classify_rust_tier(file_path: &str) -> RustCrateTier {
    if file_path.contains("/core/") {
        RustCrateTier::Core
    } else if file_path.contains("/capability/") {
        RustCrateTier::Capability
    } else if file_path.contains("/cli/")
        || file_path.contains("/hooks/")
        || file_path.contains("/writers/")
        || file_path.contains("/senders/")
        || file_path.contains("/daemons/")
        || file_path.contains("/interceptors/")
        || file_path.contains("/readers/")
        || file_path.contains("/text_writers/")
        || file_path.contains("/binary_readers/")
        || file_path.contains("/binary_writers/")
        || file_path.contains("/presence/")
        || file_path.contains("/converters/")
        || file_path.contains("/rewriters/")
        || file_path.contains("/watchers/")
        || file_path.contains("/dispatchers/")
    {
        RustCrateTier::Binary
    } else if file_path.contains("/gates/") {
        RustCrateTier::Gate
    } else {
        RustCrateTier::Unknown
    }
}

/// Check if a node is inside fn run() (binary entry point delegates to run()).
fn in_run_function(node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if ancestor.kind() == "function_item" {
            let name = ancestor
                .child_by_field_name("name")
                .map(|n| node_text(n, source))
                .unwrap_or("");
            return name == "run" || name.starts_with("run_");
        }
        current = ancestor.parent();
    }
    false
}

/// Detect println!(), print!() and dbg!() macro invocations.
///
/// Skips:
/// - Test contexts (#[test], #[cfg(test)])
/// - println!/print! inside fn main() (binary crates use stdout as their interface)
/// - println!/print! inside output functions (print_*, emit_*, display_*)
/// - println!/print! inside fn run() in binary crates (run() owns stdout interface)
///
/// dbg!() is always flagged — it is never intentional in production code.
pub fn check_no_println(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "macro_invocation") {
        let macro_node = match node.child_by_field_name("macro") {
            Some(m) => m,
            None => continue,
        };

        let macro_name = node_text(macro_node, source.source_bytes);
        let name = macro_base_name(macro_name.trim_end_matches('!'));

        if !FORBIDDEN_MACROS.contains(&name) {
            continue;
        }
        if in_test_context(node, source) {
            continue;
        }

        // println! and print! get contextual treatment; dbg! is always caught
        if name == "println" || name == "print" {
            if in_main_function(node, source.source_bytes) || in_output_function(node, source.source_bytes) {
                continue;
            }
            // In binary crates, fn run() owns the stdout interface
            if classify_rust_tier(source.file_path) == RustCrateTier::Binary
                && in_run_function(node, source.source_bytes)
            {
                continue;
            }
        }

        violations.push(violation(
            node_line(node),
            format!("{}!() is debug output — use structured logging or remove", name),
        ));
    }

    violations
}

// -------------------------------------------------------------------------
// no_clone_spam
// -------------------------------------------------------------------------

/// Context-aware clone detection.
///
/// Skips ownership transfers (struct fields, function arguments, collection
/// inserts, iterator pipelines, return position, Arc::clone, enum wrapping,
/// fallback/error closures). Flags clones in assignments and let-bindings
/// where borrowing or restructuring would avoid the allocation.
pub fn check_no_clone_spam(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "call_expression") {
        let function_node = match node.child_by_field_name("function") {
            Some(f) => f,
            None => continue,
        };

        // Arc::clone(&x) / Rc::clone(&x) — always skip (refcount bump)
        if is_arc_rc_clone(function_node, source.source_bytes) {
            continue;
        }

        // .clone() — method call on a field_expression
        if !is_method_call(function_node, "clone", source.source_bytes) {
            continue;
        }
        if in_test_context(node, source) {
            continue;
        }
        if clone_is_ownership_transfer(node, source.source_bytes) {
            continue;
        }

        violations.push(violation(
            node_line(node),
            ".clone() — consider borrowing or restructuring ownership",
        ));
    }

    violations
}

fn is_method_call(function_node: tree_sitter::Node, method: &str, source: &[u8]) -> bool {
    if function_node.kind() != "field_expression" {
        return false;
    }
    match function_node.child_by_field_name("field") {
        Some(f) => node_text(f, source) == method,
        None => false,
    }
}

fn is_arc_rc_clone(function_node: tree_sitter::Node, source: &[u8]) -> bool {
    if function_node.kind() != "scoped_identifier" {
        return false;
    }
    let text = node_text(function_node, source);
    text == "Arc::clone" || text == "Rc::clone"
}

/// Check if a closure is an argument to an iterator adaptor method.
fn is_iterator_adaptor_closure(closure: tree_sitter::Node, source: &[u8]) -> bool {
    let args = match closure.parent() {
        Some(n) if n.kind() == "arguments" => n,
        _ => return false,
    };
    let call = match args.parent() {
        Some(n) if n.kind() == "call_expression" => n,
        _ => return false,
    };
    is_method_call_to(call, source, &["map", "filter_map", "flat_map", "for_each", "and_then"])
}

/// Check if a closure is an argument to a fallback, error, or collection entry method.
fn is_fallback_or_error_closure(closure: tree_sitter::Node, source: &[u8]) -> bool {
    let args = match closure.parent() {
        Some(n) if n.kind() == "arguments" => n,
        _ => return false,
    };
    let call = match args.parent() {
        Some(n) if n.kind() == "call_expression" => n,
        _ => return false,
    };
    is_method_call_to(call, source, &[
        "unwrap_or_else", "ok_or_else", "map_err", "or_else",
        "or_insert_with", "get_or_insert_with",
    ])
}

fn is_method_call_to(call: tree_sitter::Node, source: &[u8], methods: &[&str]) -> bool {
    let func = match call.child_by_field_name("function") {
        Some(f) if f.kind() == "field_expression" => f,
        _ => return false,
    };
    let field = match func.child_by_field_name("field") {
        Some(f) => f,
        None => return false,
    };
    methods.contains(&node_text(field, source))
}

/// Check if target is within the last expression of a block.
fn is_last_expression_in_block(block: tree_sitter::Node, target: tree_sitter::Node) -> bool {
    let mut cursor = block.walk();
    match block.named_children(&mut cursor).last() {
        Some(last) => {
            target.start_byte() >= last.start_byte()
                && target.end_byte() <= last.end_byte()
        }
        None => false,
    }
}

/// Check if a block is in a value-producing position, handling if-else branches.
///
/// A block produces a value when:
/// 1. It's the body of a function, closure, or match arm (existing implicit return)
/// 2. It's a branch of an if-else expression that is itself consumed by an
///    ownership-requiring context (let binding, return, function argument, etc.)
fn is_value_producing_position(block: tree_sitter::Node, target: tree_sitter::Node) -> bool {
    // Must be within the last expression of the block
    if !is_last_expression_in_block(block, target) {
        return false;
    }

    match block.parent() {
        // Direct return contexts (existing logic)
        Some(p) if matches!(p.kind(), "function_item" | "match_arm" | "closure_expression") => {
            true
        }
        // If-else branches: the block produces a value if the outermost
        // if_expression is in a value-consuming position.
        Some(p) if p.kind() == "if_expression" || p.kind() == "else_clause" => {
            // Walk up through the if/else chain to find the outermost if_expression
            let mut if_expr = p;
            loop {
                match if_expr.parent() {
                    Some(parent) if parent.kind() == "else_clause" => {
                        if_expr = parent;
                    }
                    Some(parent) if parent.kind() == "if_expression" && if_expr.kind() == "else_clause" => {
                        if_expr = parent;
                    }
                    _ => break,
                }
            }
            // Ensure we landed on the outermost if_expression
            if if_expr.kind() == "else_clause" {
                if let Some(parent) = if_expr.parent() {
                    if parent.kind() == "if_expression" {
                        if_expr = parent;
                    }
                }
            }

            // Determine the consumption context — walk through expression_statement
            // wrapper if present (tree-sitter wraps top-level if-expressions in it)
            let consumer = if_expr.parent();
            let (check_node, consumer) = match consumer {
                Some(es) if es.kind() == "expression_statement" => (es, es.parent()),
                _ => (if_expr, consumer),
            };

            match consumer {
                Some(parent) => {
                    let kind = parent.kind();
                    // Direct value consumers
                    if matches!(kind,
                        "let_declaration" | "assignment_expression" | "return_expression"
                        | "field_initializer" | "arguments" | "call_expression"
                        | "match_arm" | "tuple_expression" | "array_expression"
                    ) {
                        return true;
                    }
                    // Tail position: if-else is the last expression in a block.
                    // Recurse: the block itself might be in a value-producing
                    // position (e.g., nested if-else inside another if-else).
                    if kind == "block" {
                        return is_value_producing_position(parent, check_node);
                    }
                    false
                }
                None => false,
            }
        }
        _ => false,
    }
}

/// The one ownership-context test, shared by `no_clone_spam` and `no_string_abuse`.
///
/// Receives the allocation call node — a `.clone()`, `.to_string()`, `.to_owned()`,
/// `.into()` or `String::from` call — and walks up its AST parents to decide
/// whether the value flows into a context that requires ownership (struct field,
/// function argument, return, match arm). A change here changes both checks.
fn clone_is_ownership_transfer(clone_call: tree_sitter::Node, source: &[u8]) -> bool {
    let mut current = clone_call.parent();
    while let Some(node) = current {
        match node.kind() {
            // Struct literal field — building owned struct from borrowed data
            "field_initializer" => return true,

            // Explicit return
            "return_expression" => return true,

            // Match arm value — the clone IS the arm's output (no block wrapper)
            "match_arm" => return true,

            // Call expression — allocation consumed by any function/method call.
            // The callee takes ownership: Type::new(x.clone()), method(x.clone()),
            // free_fn(x.clone()), collection.insert(x.clone()) all need owned values.
            "call_expression" => return true,

            // Closure — skip if passed to an iterator adaptor or fallback/error combinator
            "closure_expression" => {
                return is_iterator_adaptor_closure(node, source)
                    || is_fallback_or_error_closure(node, source);
            }

            // Block — check for implicit return or if-else value-producing position
            "block" => return is_value_producing_position(node, clone_call),

            // Transparent wrappers — keep walking up
            "arguments" | "parenthesized_expression" | "reference_expression"
            | "try_expression" | "type_cast_expression" | "assignment_expression"
            | "tuple_expression" | "array_expression" => {
                current = node.parent();
                continue;
            }

            // let mut x = val.clone() — cloning to mutate is intentional
            "let_declaration" => {
                let mut cursor = node.walk();
                return node.children(&mut cursor)
                    .any(|c| c.kind() == "mutable_specifier");
            }

            // Anything else — not a recognized transfer
            _ => return false,
        }
    }
    false
}

// -------------------------------------------------------------------------
// no_string_abuse
// -------------------------------------------------------------------------

/// Detect wasteful String construction from string literals.
///
/// Catches "literal".to_string(), String::from("literal"), "literal".to_owned().
/// These are LLM patterns — allocating heap strings when &str or const would suffice.
pub fn check_no_string_abuse(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "call_expression") {
        if in_test_context(node, source) {
            continue;
        }
        if let Some(msg) = detect_literal_method_abuse(node, source.source_bytes) {
            // Same ownership taxonomy as clone — if the allocation flows into a
            // struct field, return, collection method, or enum constructor, the
            // owned String is required and flagging it is a false positive.
            if !clone_is_ownership_transfer(node, source.source_bytes) {
                violations.push(violation(node_line(node), msg));
            }
            continue;
        }
        if detect_string_from_literal(node, source.source_bytes) {
            if !clone_is_ownership_transfer(node, source.source_bytes) {
                violations.push(violation(
                    node_line(node),
                    "String::from(\"...\") allocates — use &str or const",
                ));
            }
        }
    }

    violations
}

/// "literal".to_string() / "literal".to_owned() / "literal".into()
fn detect_literal_method_abuse(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let func = node.child_by_field_name("function")?;
    if func.kind() != "field_expression" {
        return None;
    }
    let field = func.child_by_field_name("field")?;
    let value = func.child_by_field_name("value")?;
    let method = node_text(field, source);
    if !matches!(method, "to_string" | "to_owned" | "into") {
        return None;
    }
    if value.kind() != "string_literal" {
        return None;
    }
    Some(format!("\"...\".{method}() allocates — use &str or const"))
}

/// String::from("literal")
fn detect_string_from_literal(node: tree_sitter::Node, source: &[u8]) -> bool {
    let func = match node.child_by_field_name("function") {
        Some(f) if f.kind() == "scoped_identifier" => f,
        _ => return false,
    };
    if node_text(func, source) != "String::from" {
        return false;
    }
    let args = match node.child_by_field_name("arguments") {
        Some(a) => a,
        None => return false,
    };
    let mut cursor = args.walk();
    let has_string = args.named_children(&mut cursor).any(|c| c.kind() == "string_literal");
    has_string
}

// -------------------------------------------------------------------------
// no_pub_overuse
// -------------------------------------------------------------------------

/// Detect when every function in a file is pub.
///
/// LLMs make everything pub to avoid compiler errors about unused/unreachable
/// code, rather than designing proper module boundaries.
/// Only fires when there are multiple functions and ALL are pub.
pub fn check_no_pub_overuse(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    // lib.rs and mod.rs are structural entry points — pub is expected
    let filename = source.file_path.rsplit('/').next().unwrap_or(source.file_path);
    if filename == "lib.rs" || filename == "mod.rs" {
        return Vec::new();
    }

    let mut total_fns = 0usize;
    let mut pub_fns = 0usize;

    for node in find_nodes_by_type(source.tree.root_node(), "function_item") {
        // Skip functions inside impl blocks (methods) and test contexts
        if let Some(parent) = node.parent() {
            if parent.kind() == "impl_item" || parent.kind() == "declaration_list" {
                continue;
            }
        }
        if in_test_context(node, source) {
            continue;
        }

        total_fns += 1;

        if is_pub_unrestricted(node, source.source_bytes) {
            pub_fns += 1;
        }
    }

    // Only flag when there are enough functions AND all are bare pub
    if total_fns >= config.min_functions_for_pub_check && pub_fns == total_fns {
        vec![violation(
            1,
            "every function is pub — design module boundaries instead".to_string(),
        )]
    } else {
        Vec::new()
    }
}

/// Check if a node has bare `pub` visibility (not `pub(crate)` or `pub(super)`).
/// Restricted visibility shows intentional boundary design and should not be flagged.
fn is_pub_unrestricted(node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            let text = node_text(child, source);
            // bare "pub" vs "pub(crate)", "pub(super)", "pub(in ...)"
            return text == "pub";
        }
    }
    false
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

    fn parse_at(path: &'static str, code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source_rust(path, source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside, &crate::STATISTICS)
    }

    // -- no_unwrap --

    #[test]
    fn unwrap_caught() {
        let parsed = parse("fn main() { let x: Option<i32> = Some(1); x.unwrap(); }");
        let violations = check_no_unwrap(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("unwrap"));
    }

    #[test]
    fn expect_caught() {
        let parsed = parse(r#"fn main() { let x: Option<i32> = Some(1); x.expect("bad"); }"#);
        let violations = check_no_unwrap(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("expect"));
    }

    #[test]
    fn unwrap_or_ok() {
        let parsed = parse("fn main() { let x: Option<i32> = Some(1); x.unwrap_or(0); }");
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn unwrap_or_default_ok() {
        let parsed = parse("fn main() { let x: Option<i32> = Some(1); x.unwrap_or_default(); }");
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn unwrap_in_test_skipped() {
        let parsed = parse(
            r#"
            #[cfg(test)]
            mod tests {
                #[test]
                fn it_works() {
                    let x: Option<i32> = Some(1);
                    x.unwrap();
                }
            }
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn unwrap_in_test_function_skipped() {
        let parsed = parse(
            r#"
            #[test]
            fn it_works() {
                let x: Option<i32> = Some(1);
                x.unwrap();
            }
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn unwrap_outside_test_caught() {
        let parsed = parse(
            r#"
            fn production_code() {
                let x: Option<i32> = Some(1);
                x.unwrap();
            }

            #[cfg(test)]
            mod tests {
                #[test]
                fn it_works() {
                    let x: Option<i32> = Some(1);
                    x.unwrap();
                }
            }
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn unwrap_in_static_lazy_lock_ok() {
        let parsed = parse(
            r#"
            use std::sync::LazyLock;
            use regex::Regex;
            static PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+$").unwrap());
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty(), "unwrap in LazyLock static should be skipped");
    }

    #[test]
    fn expect_in_static_lazy_lock_ok() {
        let parsed = parse(
            r#"
            use std::sync::LazyLock;
            static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
                serde_json::from_str(SCHEMA_JSON).expect("embedded schema must be valid JSON")
            });
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty(), "expect in LazyLock static should be skipped");
    }

    #[test]
    fn unwrap_in_const_ok() {
        let parsed = parse(
            r#"
            const VALUE: i32 = Some(42).unwrap();
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty(), "unwrap in const initializer should be skipped");
    }

    #[test]
    fn unwrap_in_static_multiline_ok() {
        let parsed = parse(
            r#"
            use std::sync::LazyLock;
            use regex::Regex;
            static PYRIGHT_RE: LazyLock<Regex> =
                LazyLock::new(|| Regex::new(r"test").unwrap());
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty(), "unwrap in multiline LazyLock static should be skipped: {:?}",
            violations.iter().map(|v| &v.message).collect::<Vec<_>>());
    }

    #[test]
    fn unwrap_in_function_still_caught() {
        let parsed = parse(
            r#"
            use std::sync::LazyLock;
            use regex::Regex;
            static PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+$").unwrap());
            fn production_code() {
                let x: Option<i32> = Some(1);
                x.unwrap();
            }
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "only function unwrap should be caught, not static");
    }

    #[test]
    fn expect_in_get_or_init_ok() {
        let parsed = parse(
            r#"
            use std::sync::OnceLock;
            struct Schema { validator: OnceLock<String> }
            impl Schema {
                fn get_validator(&self) -> &String {
                    self.validator.get_or_init(|| {
                        let data = "test".to_string();
                        data.parse().expect("must be valid")
                    })
                }
            }
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty(), "expect in get_or_init closure should be skipped: {:?}",
            violations.iter().map(|v| &v.message).collect::<Vec<_>>());
    }

    #[test]
    fn unwrap_in_regular_closure_still_caught() {
        let parsed = parse(
            r#"
            fn process() {
                let items: Vec<i32> = vec![1, 2, 3];
                let results: Vec<i32> = items.iter().map(|x| {
                    Some(*x).unwrap()
                }).collect();
            }
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "unwrap in regular closure should still be caught");
    }

    #[test]
    fn unwrap_or_else_ok() {
        let parsed = parse("fn main() { let x: Option<i32> = Some(1); x.unwrap_or_else(|| 0); }");
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_unwrap: panicking macros --

    const PANICKING_INVOCATIONS: &[(&str, &str)] = &[
        ("panic", r#"panic!("bad state")"#),
        ("unreachable", "unreachable!()"),
        ("todo", "todo!()"),
        ("unimplemented", "unimplemented!()"),
    ];

    #[test]
    fn panicking_macros_caught_in_plain_fn() {
        for (name, invocation) in PANICKING_INVOCATIONS {
            let parsed = parse(&format!("fn production_code() -> i32 {{ {invocation} }}"));
            let violations = check_no_unwrap(&parsed, &default_config());
            assert_eq!(violations.len(), 1, "{name}! should be caught");
            assert!(violations[0].message.contains(name));
            assert!(!violations[0].message.contains(|c: char| c.is_ascii_digit()));
        }
    }

    #[test]
    fn panicking_macros_in_test_module_skipped() {
        for (name, invocation) in PANICKING_INVOCATIONS {
            let parsed = parse(&format!(
                "#[cfg(test)]\nmod tests {{\n    fn helper() -> i32 {{ {invocation} }}\n}}\n"
            ));
            let violations = check_no_unwrap(&parsed, &default_config());
            assert!(violations.is_empty(), "{name}! in #[cfg(test)] mod should be skipped");
        }
    }

    #[test]
    fn panicking_macros_in_test_function_skipped() {
        for (name, invocation) in PANICKING_INVOCATIONS {
            let parsed = parse(&format!("#[test]\nfn it_works() {{ {invocation} }}\n"));
            let violations = check_no_unwrap(&parsed, &default_config());
            assert!(violations.is_empty(), "{name}! in #[test] fn should be skipped");
        }
    }

    #[test]
    fn panic_in_static_lazy_lock_ok() {
        let parsed = parse(
            r#"
            use std::sync::LazyLock;
            static TABLE: LazyLock<Vec<i32>> = LazyLock::new(|| {
                if SOURCE.is_empty() { panic!("embedded table must not be empty") }
                Vec::new()
            });
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty(), "panic! in LazyLock static should be skipped");
    }

    #[test]
    fn non_panicking_macro_ok() {
        let parsed = parse(r#"fn production_code() -> String { format!("{}", 1) }"#);
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- macros by the last segment of their path --

    #[test]
    fn a_macro_s_base_name_is_the_last_segment_of_its_path() {
        assert_eq!(macro_base_name("std::panic"), "panic");
        assert_eq!(macro_base_name("::core::unreachable"), "unreachable");
        assert_eq!(macro_base_name("println"), "println");
    }

    #[test]
    fn a_macro_s_base_name_is_not_a_suffix_of_its_last_segment() {
        assert_eq!(macro_base_name("my_panic"), "my_panic");
        assert_eq!(macro_base_name("helpers::not_println"), "not_println");
    }

    #[test]
    fn panicking_macros_invoked_by_a_qualified_path_are_caught() {
        for (name, invocation) in [
            ("panic", r#"std::panic!("bad state")"#),
            ("unreachable", "core::unreachable!()"),
            ("todo", "::std::todo!()"),
            ("unimplemented", "std::unimplemented!()"),
        ] {
            let parsed = parse(&format!("fn production_code() -> i32 {{ {invocation} }}"));
            let violations = check_no_unwrap(&parsed, &default_config());
            assert_eq!(violations.len(), 1, "{invocation} should be caught");
            assert!(violations[0].message.contains(&format!("{name}!()")), "{}", violations[0].message);
        }
    }

    #[test]
    fn a_macro_that_only_ends_in_a_panicking_name_is_ok() {
        for invocation in [r#"my_panic!("bad state")"#, "helpers::not_unreachable!()", "todo_list!()"] {
            let parsed = parse(&format!("fn production_code() -> i32 {{ {invocation} }}"));
            let violations = check_no_unwrap(&parsed, &default_config());
            assert!(violations.is_empty(), "{invocation} is not a panicking macro");
        }
    }

    #[test]
    fn print_macros_invoked_by_a_qualified_path_are_caught() {
        for (name, invocation) in [
            ("println", r#"std::println!("hello")"#),
            ("print", r#"std::print!("hello")"#),
            ("dbg", "std::dbg!(1)"),
        ] {
            let parsed = parse(&format!("fn process() {{ {invocation}; }}"));
            let violations = check_no_println(&parsed, &default_config());
            assert_eq!(violations.len(), 1, "{invocation} should be caught");
            assert!(violations[0].message.contains(&format!("{name}!()")), "{}", violations[0].message);
        }
    }

    #[test]
    fn a_macro_that_only_ends_in_a_print_name_is_ok() {
        for invocation in [r#"not_println!("hello")"#, r#"log::my_print!("hello")"#, "my_dbg!(1)"] {
            let parsed = parse(&format!("fn process() {{ {invocation}; }}"));
            let violations = check_no_println(&parsed, &default_config());
            assert!(violations.is_empty(), "{invocation} is not a print macro");
        }
    }

    // -- test context by path --

    const HELPER_WITH_UNWRAP: &str = "fn helper() -> i32 { let x: Option<i32> = Some(1); x.unwrap() }";

    #[test]
    fn a_file_under_a_crate_s_tests_directory_is_a_test_file() {
        assert!(path_is_test_file("/x/crate/tests/cli.rs"));
        assert!(path_is_test_file("/x/crate/tests/common/mod.rs"));
    }

    #[test]
    fn a_file_under_a_src_directory_below_tests_or_under_no_tests_directory_is_not_a_test_file() {
        assert!(!path_is_test_file("/x/tests/fixtures/crate/src/lib.rs"));
        assert!(!path_is_test_file("/x/tests/src/lib.rs"));
        assert!(!path_is_test_file("/x/crate/src/lib.rs"));
        assert!(!path_is_test_file("/x/tests_data/src/lib.rs"));
    }

    #[test]
    fn unwrap_in_a_fixture_crate_s_src_below_tests_caught() {
        let parsed = parse_at("/x/tests/fixtures/crate/src/lib.rs", HELPER_WITH_UNWRAP);
        let violations = check_no_unwrap(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "a fixture crate's src/ is production code");
    }

    #[test]
    fn tests_directory_segment_is_a_test_file() {
        assert!(path_is_test_file("/x/readers/read_json/tests/cli_contract.rs"));
        assert!(!path_is_test_file("/x/readers/read_json/src/lib.rs"));
        assert!(!path_is_test_file("/x/tests_data/src/lib.rs"));
    }

    #[test]
    fn unwrap_in_integration_test_helper_skipped() {
        let parsed = parse_at("/x/readers/read_json/tests/cli_contract.rs", HELPER_WITH_UNWRAP);
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty(), "helper fn in tests/ is test code");
    }

    #[test]
    fn unwrap_in_src_file_caught() {
        let parsed = parse_at("/x/readers/read_json/src/lib.rs", HELPER_WITH_UNWRAP);
        let violations = check_no_unwrap(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn unwrap_under_directory_merely_starting_with_tests_caught() {
        let parsed = parse_at("/x/tests_data/src/lib.rs", HELPER_WITH_UNWRAP);
        let violations = check_no_unwrap(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- no_println --

    #[test]
    fn print_caught() {
        let parsed = parse(r#"fn process() { print!("hello"); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("print!"));
    }

    #[test]
    fn print_in_fn_main_ok() {
        let parsed = parse(r#"fn main() { print!("{}", 1); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty(), "print inside fn main() should be allowed");
    }

    #[test]
    fn print_in_run_function_binary_crate_ok() {
        let parsed = parse_at(
            "/nornir/cli/syn_cli/src/main.rs",
            "fn run() -> Result<i32, String> { print!(\"output\"); Ok(0) }",
        );
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty(), "print in fn run() of binary crate should be allowed");
    }

    #[test]
    fn print_in_run_function_library_crate_caught() {
        let parsed = parse_at("/nornir/core/text_core/src/lib.rs", "fn run() { print!(\"debug\"); }");
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn eprint_and_eprintln_ok() {
        let parsed = parse(r#"fn process() { eprint!("partial"); eprintln!("error"); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty(), "eprint!/eprintln! are stderr output, not debug prints");
    }

    #[test]
    fn println_caught() {
        let parsed = parse(r#"fn process() { println!("hello"); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("println"));
    }

    #[test]
    fn eprintln_ok() {
        let parsed = parse(r#"fn main() { eprintln!("error"); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty(), "eprintln is legitimate error output for CLI tools");
    }

    #[test]
    fn dbg_caught() {
        let parsed = parse("fn main() { let x = 1; dbg!(x); }");
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("dbg"));
    }

    #[test]
    fn println_in_test_ok() {
        let parsed = parse(
            r#"
            #[cfg(test)]
            mod tests {
                #[test]
                fn it_works() {
                    println!("debug");
                }
            }
            "#,
        );
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn println_in_fn_main_ok() {
        let source: &'static [u8] =
            Box::leak(b"fn main() { println!(\"starting\"); }".to_vec().into_boxed_slice());
        let parsed = build_parsed_source_rust("/app/src/main.rs", source).unwrap();
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty(), "println inside fn main() should be allowed");
    }

    #[test]
    fn println_in_helper_within_main_rs_caught() {
        let source: &'static [u8] = Box::leak(
            b"fn helper() { println!(\"debug\"); }\nfn main() { helper(); }"
                .to_vec()
                .into_boxed_slice(),
        );
        let parsed = build_parsed_source_rust("/app/src/main.rs", source).unwrap();
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "println in helper within main.rs must be caught");
    }

    #[test]
    fn println_in_fn_main_non_binary_ok() {
        // fn main() is exempt regardless of file path
        let parsed = parse(r#"fn main() { println!("starting"); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty(), "println inside fn main() should be allowed even in non-binary files");
    }

    #[test]
    fn dbg_in_main_rs_still_caught() {
        let source: &'static [u8] =
            Box::leak(b"fn main() { let x = 1; dbg!(x); }".to_vec().into_boxed_slice());
        let parsed = build_parsed_source_rust("/app/src/main.rs", source).unwrap();
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "dbg! should be caught even in main.rs");
    }

    #[test]
    fn println_in_output_function_ok() {
        let parsed = parse(r#"fn print_help() { println!("Usage: tool [options]"); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty(), "println in print_help should be skipped");
    }

    #[test]
    fn println_in_emit_function_ok() {
        let parsed = parse(r#"fn emit_report() { println!("Report:"); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty(), "println in emit_report should be skipped");
    }

    #[test]
    fn println_in_regular_function_caught() {
        let parsed = parse(r#"fn process() { println!("debug"); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "println in regular function should be caught");
    }

    #[test]
    fn format_macro_ok() {
        let parsed = parse(r#"fn main() { let s = format!("hello {}", 42); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_clone_spam --

    #[test]
    fn bare_clone_caught() {
        let parsed = parse("fn main() { let s = String::new(); let s2 = s.clone(); }");
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("clone"));
    }

    #[test]
    fn clone_in_test_ok() {
        let parsed = parse(
            r#"
            #[cfg(test)]
            mod tests {
                #[test]
                fn it_works() {
                    let s = String::new();
                    let s2 = s.clone();
                }
            }
            "#,
        );
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn no_clone_ok() {
        let parsed = parse("fn main() { let x = 42; let y = x; }");
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- clone skip rules --

    #[test]
    fn clone_in_struct_field_ok() {
        let code = r#"
            struct Issue { tool: String }
            fn build(item: &Issue) -> Issue {
                Issue { tool: item.tool.clone() }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "struct field clone should be skipped");
    }

    #[test]
    fn clone_in_collection_insert_ok() {
        let code = r#"
            fn build() {
                let mut map = std::collections::HashMap::new();
                let key = String::from("k");
                map.insert(key.clone(), 42);
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "collection insert clone should be skipped");
    }

    #[test]
    fn clone_in_vec_push_ok() {
        let code = r#"
            fn build() {
                let mut items: Vec<String> = Vec::new();
                let val = String::from("v");
                items.push(val.clone());
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "vec push clone should be skipped");
    }

    #[test]
    fn clone_in_iter_map_ok() {
        let code = r#"
            fn transform(items: &[String]) -> Vec<String> {
                items.iter().map(|s| s.clone()).collect()
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "iterator map clone should be skipped");
    }

    #[test]
    fn clone_in_return_ok() {
        let code = r#"
            fn get_name(data: &String) -> String {
                return data.clone();
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "explicit return clone should be skipped");
    }

    #[test]
    fn clone_in_implicit_return_ok() {
        let code = r#"
            fn get_name(data: &String) -> String {
                data.clone()
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "implicit return clone should be skipped");
    }

    #[test]
    fn arc_clone_ok() {
        let code = r#"
            fn share(data: &std::sync::Arc<String>) -> std::sync::Arc<String> {
                Arc::clone(data)
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "Arc::clone should be skipped");
    }

    #[test]
    fn clone_in_some_ok() {
        let code = r#"
            fn wrap(path: &String) -> Option<String> {
                Some(path.clone())
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "Some() clone should be skipped");
    }

    #[test]
    fn clone_in_match_arm_ok() {
        let code = r#"
            fn get_value(opt: &Option<String>) -> String {
                match opt {
                    Some(s) => s.clone(),
                    None => String::new(),
                }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "clone in match arm should be skipped");
    }

    #[test]
    fn clone_in_match_arm_block_ok() {
        let code = r#"
            fn get_value(opt: &Option<String>) -> String {
                match opt {
                    Some(s) => {
                        let _len = s.len();
                        s.clone()
                    }
                    None => String::new(),
                }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "clone as last expr in match arm block should be skipped");
    }

    #[test]
    fn clone_in_tuple_ok() {
        let code = r#"
            fn build(name: &String, value: &String) -> (String, String) {
                (name.clone(), value.clone())
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "clone in tuple expression should be skipped");
    }

    #[test]
    fn clone_in_ok_variant_ok() {
        let code = r#"
            fn wrap(data: &String) -> Result<String, ()> {
                Ok(data.clone())
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "Ok() clone should be skipped");
    }

    #[test]
    fn clone_as_function_arg_ok() {
        let code = r#"
            fn setup(path: String) {}
            fn main() {
                let config_path = String::from("/tmp/socket");
                setup(config_path.clone());
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "clone passed as function argument should be skipped");
    }

    #[test]
    fn clone_in_type_constructor_ok() {
        let code = r#"
            fn convert(name: &String) {
                let val = Value::String(name.clone());
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "clone in type constructor should be skipped");
    }

    #[test]
    fn clone_in_or_insert_with_ok() {
        let code = r#"
            fn group(items: &[Issue]) {
                let mut groups = std::collections::HashMap::new();
                for item in items {
                    groups.entry("key").or_insert_with(|| item.clone());
                }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "clone in or_insert_with closure should be skipped");
    }

    #[test]
    fn clone_in_assignment_still_caught() {
        let code = r#"
            struct Data { field: String }
            fn update(data: &mut Data, source: &Data) {
                data.field = source.field.clone();
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "clone in assignment should still be caught");
    }

    // -- no_string_abuse --

    #[test]
    fn literal_to_string_caught() {
        let parsed = parse(r#"fn main() { let s = "hello".to_string(); }"#);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("to_string"));
    }

    #[test]
    fn literal_to_owned_caught() {
        let parsed = parse(r#"fn main() { let s = "hello".to_owned(); }"#);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("to_owned"));
    }

    #[test]
    fn string_from_literal_caught() {
        let parsed = parse(r#"fn main() { let s = String::from("hello"); }"#);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("String::from"));
    }

    #[test]
    fn variable_to_string_ok() {
        let parsed = parse("fn main() { let x = 42; let s = x.to_string(); }");
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn string_abuse_in_test_ok() {
        let parsed = parse(
            r#"
            #[cfg(test)]
            mod tests {
                #[test]
                fn it_works() {
                    let s = "hello".to_string();
                }
            }
            "#,
        );
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn to_string_in_struct_field_ok() {
        let code = r#"
            struct Config { name: String }
            fn build() -> Config {
                Config { name: "default".to_string() }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "to_string in struct field should be skipped");
    }

    #[test]
    fn into_in_map_insert_ok() {
        let code = r#"
            fn build() {
                let mut map = std::collections::HashMap::new();
                map.insert("key".into(), 42);
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "into() in map insert should be skipped");
    }

    #[test]
    fn to_string_in_return_ok() {
        let code = r#"
            fn default_name() -> String {
                return "unknown".to_string();
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "to_string in return should be skipped");
    }

    #[test]
    fn to_string_in_some_ok() {
        let code = r#"
            fn maybe_name() -> Option<String> {
                Some("default".to_string())
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "to_string in Some() should be skipped");
    }

    #[test]
    fn string_from_in_struct_field_ok() {
        let code = r#"
            struct Config { name: String }
            fn build() -> Config {
                Config { name: String::from("default") }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "String::from in struct field should be skipped");
    }

    #[test]
    fn to_string_as_function_arg_ok() {
        let code = r#"
            fn setup(name: String) {}
            fn main() {
                setup("default".to_string());
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "to_string passed as function arg should be skipped");
    }

    #[test]
    fn into_in_or_insert_with_closure_ok() {
        let code = r#"
            fn build() {
                let mut map = std::collections::HashMap::new();
                map.entry("key").or_insert_with(|| "default".to_string());
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "to_string in or_insert_with closure should be skipped");
    }

    #[test]
    fn to_string_in_let_binding_still_caught() {
        let code = r#"
            fn main() {
                let name = "hello".to_string();
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "to_string in bare let binding should still be caught");
    }

    // -- no_pub_overuse --

    #[test]
    fn all_pub_caught() {
        let parsed = parse(
            "pub fn a() {} pub fn b() {} pub fn c() {} pub fn d() {}",
        );
        let violations = check_no_pub_overuse(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("pub"));
    }

    #[test]
    fn mixed_visibility_ok() {
        let parsed = parse(
            "pub fn a() {} fn b() {} pub fn c() {} fn d() {}",
        );
        let violations = check_no_pub_overuse(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn few_pub_fns_ok() {
        let parsed = parse("pub fn a() {} pub fn b() {}");
        let violations = check_no_pub_overuse(&parsed, &default_config());
        assert!(violations.is_empty()); // Only 2 functions, threshold is 4
    }

    #[test]
    fn all_pub_in_test_ok() {
        let parsed = parse(
            r#"
            #[cfg(test)]
            mod tests {
                pub fn a() {}
                pub fn b() {}
                pub fn c() {}
                pub fn d() {}
            }
            "#,
        );
        let violations = check_no_pub_overuse(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn pub_crate_shows_boundary_design() {
        let source: &'static [u8] = Box::leak(
            b"pub fn a() {} pub fn b() {} pub fn c() {} pub(crate) fn d() {}".to_vec().into_boxed_slice(),
        );
        let parsed = build_parsed_source_rust("/test/file.rs", source).unwrap();
        let violations = check_no_pub_overuse(&parsed, &default_config());
        assert!(violations.is_empty()); // pub(crate) means intentional scoping
    }

    #[test]
    fn lib_rs_exempt() {
        let source: &'static [u8] = Box::leak(
            b"pub fn a() {} pub fn b() {} pub fn c() {} pub fn d() {}".to_vec().into_boxed_slice(),
        );
        let parsed = build_parsed_source_rust("/test/lib.rs", source).unwrap();
        let violations = check_no_pub_overuse(&parsed, &default_config());
        assert!(violations.is_empty()); // lib.rs is a crate entry point
    }

    #[test]
    fn mod_rs_exempt() {
        let source: &'static [u8] = Box::leak(
            b"pub fn a() {} pub fn b() {} pub fn c() {} pub fn d() {}".to_vec().into_boxed_slice(),
        );
        let parsed = build_parsed_source_rust("/test/mod.rs", source).unwrap();
        let violations = check_no_pub_overuse(&parsed, &default_config());
        assert!(violations.is_empty()); // mod.rs is a module entry point
    }

    // -- if-else value-producing position (Change 1) --

    #[test]
    fn to_string_in_if_else_let_binding_ok() {
        let code = r#"
            fn format_refs(refs: &[String]) -> String {
                let display = if refs.is_empty() {
                    "none".to_string()
                } else {
                    refs.join(", ")
                };
                display
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "to_string in if-else let binding should be skipped");
    }

    #[test]
    fn clone_in_if_else_let_binding_ok() {
        let code = r#"
            fn pick(flag: bool, owned: &String, fallback: &String) -> String {
                let result = if flag {
                    owned.clone()
                } else {
                    fallback.clone()
                };
                result
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "clone in if-else let binding should be skipped");
    }

    #[test]
    fn to_string_in_else_if_chain_ok() {
        let code = r#"
            fn normalize(path: &str) -> String {
                if path.starts_with("/") {
                    format!("/{}", path)
                } else if path.is_empty() {
                    ".".to_string()
                } else {
                    path.to_string()
                }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "to_string in else-if implicit return should be skipped");
    }

    #[test]
    fn to_string_in_if_else_function_arg_ok() {
        let code = r#"
            fn process(paths: &mut Vec<String>, prefix: &str) {
                paths.push(if prefix.is_empty() {
                    "(root)".to_string()
                } else {
                    prefix.to_string()
                });
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "to_string in if-else as function arg should be skipped");
    }

    #[test]
    fn to_string_in_nested_if_else_ok() {
        let code = r#"
            fn workspace(path: &str) -> String {
                if let Some(relative) = path.strip_prefix("/base/") {
                    let trimmed = relative.trim_end_matches('/');
                    if trimmed.is_empty() {
                        "@".to_string()
                    } else {
                        format!("@{}", trimmed)
                    }
                } else {
                    "@".to_string()
                }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "to_string in nested if-else tail position should be skipped: {:?}",
            violations.iter().map(|v| (v.line, &v.message)).collect::<Vec<_>>());
    }

    #[test]
    fn to_string_in_plain_let_binding_still_caught() {
        let code = r#"
            fn process() {
                let name = "hello".to_string();
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "to_string in plain let binding must still be caught");
    }

    #[test]
    fn to_string_in_if_body_not_value_position_still_caught() {
        let code = r#"
            fn process(flag: bool) {
                if flag {
                    let name = "hello".to_string();
                }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "to_string in if-body let binding (not value position) must still be caught");
    }

    // -- tier-aware println (Change 2) --

    #[test]
    fn println_in_run_function_binary_crate_ok() {
        let source: &'static [u8] = Box::leak(
            b"fn run() -> Result<i32, String> { println!(\"output\"); Ok(0) }"
                .to_vec().into_boxed_slice(),
        );
        let parsed = build_parsed_source_rust("/nornir/cli/syn_cli/src/main.rs", source).unwrap();
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty(), "println in run() of binary crate should be allowed");
    }

    #[test]
    fn println_in_run_prefix_function_binary_crate_ok() {
        let source: &'static [u8] = Box::leak(
            b"fn run_file() -> Result<(), String> { println!(\"output\"); Ok(()) }"
                .to_vec().into_boxed_slice(),
        );
        let parsed = build_parsed_source_rust("/nornir/cli/saga_cli/src/main.rs", source).unwrap();
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty(), "println in run_file() of binary crate should be allowed");
    }

    #[test]
    fn println_in_run_function_library_crate_caught() {
        let source: &'static [u8] = Box::leak(
            b"fn run() { println!(\"debug\"); }"
                .to_vec().into_boxed_slice(),
        );
        let parsed = build_parsed_source_rust("/nornir/core/some_core/src/lib.rs", source).unwrap();
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "println in run() of library crate must still be caught");
    }

    #[test]
    fn println_in_helper_function_binary_crate_caught() {
        let source: &'static [u8] = Box::leak(
            b"fn helper() { println!(\"debug\"); }\nfn run() -> i32 { helper(); 0 }"
                .to_vec().into_boxed_slice(),
        );
        let parsed = build_parsed_source_rust("/nornir/cli/syn_cli/src/main.rs", source).unwrap();
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "println in helper within binary crate must still be caught");
    }

    #[test]
    fn dbg_in_run_function_binary_crate_still_caught() {
        let source: &'static [u8] = Box::leak(
            b"fn run() -> i32 { let x = 1; dbg!(x); 0 }"
                .to_vec().into_boxed_slice(),
        );
        let parsed = build_parsed_source_rust("/nornir/cli/syn_cli/src/main.rs", source).unwrap();
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "dbg! must always be caught, even in binary run()");
    }

    #[test]
    fn tier_classification() {
        assert_eq!(classify_rust_tier("/nornir/core/text_core/src/lib.rs"), RustCrateTier::Core);
        assert_eq!(classify_rust_tier("/nornir/capability/hook_io/src/lib.rs"), RustCrateTier::Capability);
        assert_eq!(classify_rust_tier("/nornir/cli/syn_cli/src/main.rs"), RustCrateTier::Binary);
        assert_eq!(classify_rust_tier("/nornir/hooks/hook_pre_llm_bash/src/main.rs"), RustCrateTier::Binary);
        assert_eq!(classify_rust_tier("/nornir/writers/append_raw_jsonl/src/main.rs"), RustCrateTier::Binary);
        assert_eq!(classify_rust_tier("/nornir/gates/gate_structure_in/src/lib.rs"), RustCrateTier::Gate);
        assert_eq!(classify_rust_tier("/other/project/src/main.rs"), RustCrateTier::Unknown);
    }

    #[test]
    fn newly_listed_binary_directories_are_binary_tier() {
        let directories = [
            "readers", "text_writers", "binary_readers", "binary_writers", "presence",
            "converters", "rewriters", "watchers", "dispatchers",
        ];
        for directory in directories {
            let path = format!("/nornir/{directory}/some_tool/src/main.rs");
            assert_eq!(classify_rust_tier(&path), RustCrateTier::Binary, "{directory}");
        }
    }

    /// The nornir workspace manifest, so the tier map is checked against the
    /// directories that exist rather than against a second hand-kept list.
    const WORKSPACE_MANIFEST: &str = include_str!("../../../../Cargo.toml");

    #[test]
    fn every_workspace_member_has_a_tier() {
        let manifest: toml::Value = toml::from_str(WORKSPACE_MANIFEST).unwrap();
        let members = manifest["workspace"]["members"].as_array().unwrap();
        assert!(!members.is_empty(), "the workspace lists its members");

        let mut without_tier = Vec::new();
        for member in members {
            let member_path = member.as_str().unwrap();
            let file_path = format!("/x/{member_path}/src/lib.rs");
            if classify_rust_tier(&file_path) == RustCrateTier::Unknown {
                without_tier.push(member_path);
            }
        }
        assert!(without_tier.is_empty(), "workspace members with no tier: {without_tier:?}");
    }

    // -- let-mut clone detection (Change 3) --

    #[test]
    fn clone_into_let_mut_ok() {
        let code = r#"
            fn strip(value: &serde_json::Value) -> serde_json::Value {
                let mut stripped = value.clone();
                stripped
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "clone into let mut should be skipped");
    }

    #[test]
    fn clone_into_let_immutable_still_caught() {
        let code = r#"
            fn copy(value: &String) {
                let copied = value.clone();
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "clone into immutable let should still be caught");
    }
}
