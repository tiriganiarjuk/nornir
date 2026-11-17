//! Prohibited pattern checks.
//!
//! Checks: no_cast, no_overload, no_bare_except, no_broad_exceptions,
//! no_print, no_model_dump, no_future_annotations, init_files_empty,
//! no_dunder_all, no_nested_functions (+ no_closures),
//! no_general_lambda, no_default_factory, no_recursion.

use crate::messages;
use crate::parsing::{bound_names, find_nodes_by_type, module_names, names_for, node_field, node_line, node_text};
use crate::structures::{CheckConfig, CheckMessages, ParsedSource, Severity, Violation};

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

/// Whether a call is to the function `name`: by one of the local names the file
/// uses for it, or as an attribute of that name on any object.
fn calls_named(node: tree_sitter::Node, name: &str, local_names: &[String], source: &[u8]) -> bool {
    let func = match node.child_by_field_name("function") {
        Some(f) => f,
        None => return false,
    };
    match func.kind() {
        "identifier" => local_names.iter().any(|local| local == node_text(func, source)),
        "attribute" => {
            func.child_by_field_name("attribute")
                .is_some_and(|a| node_text(a, source) == name)
        }
        _ => false,
    }
}

pub fn check_no_cast(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let root = source.tree.root_node();
    let cast_names = names_for(root, source.source_bytes, "cast", &["typing", "typing_extensions"]);
    let mut violations = Vec::new();
    for node in find_nodes_by_type(root, "call") {
        if calls_named(node, "cast", &cast_names, source.source_bytes) {
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

/// The exception types an except clause catches, each by its last name:
/// `except A`, `except mod.A`, `except (A, B) as caught`.
fn caught_type_names<'a>(clause: tree_sitter::Node, source: &'a [u8]) -> Vec<&'a str> {
    let mut cursor = clause.walk();
    let Some(caught) = clause.named_children(&mut cursor).find(|child| child.kind() != "block") else {
        return Vec::new();
    };
    let caught = match caught.kind() {
        "as_pattern" => caught.named_child(0).unwrap_or(caught),
        _ => caught,
    };
    let members: Vec<tree_sitter::Node> = match caught.kind() {
        "tuple" | "parenthesized_expression" => {
            let mut member_cursor = caught.walk();
            caught.named_children(&mut member_cursor).collect()
        }
        _ => vec![caught],
    };
    members.into_iter().filter_map(|member| last_name(member, source)).collect()
}

/// A name, or an attribute's last segment.
fn last_name<'a>(node: tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    match node.kind() {
        "identifier" => Some(node_text(node, source)),
        "attribute" => node.child_by_field_name("attribute").map(|attribute| node_text(attribute, source)),
        _ => None,
    }
}

/// The exception types that catch whatever a program raises.
const BROAD_EXCEPTIONS: &[&str] = &["Exception", "BaseException"];

pub fn check_no_broad_exceptions(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    for node in find_nodes_by_type(source.tree.root_node(), "except_clause") {
        if let Some(name) = caught_type_names(node, source.source_bytes).into_iter().find(|name| BROAD_EXCEPTIONS.contains(name)) {
            violations.push(violation(node_line(node), format!("except {name} is too broad")));
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

// Reversed to avoid triggering text scanner. Each ends at the opening
// parenthesis, so a call with arguments is matched as one without is.
const REVERSED_MODEL_DUMP: &[&str] = &[
    "(pmud_ledom.",
    "(tcid.",
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
// no_model_bypass — a model read or changed as a bag of keys
// -------------------------------------------------------------------------

/// Builtins that go around a model's fields, each with what it does.
const BUILTIN_BYPASSES: &[(&str, &str)] = &[
    ("getattr", "getattr() reaches a field by a name held in a string"),
    ("setattr", "setattr() reaches a field by a name held in a string"),
    ("delattr", "delattr() reaches a field by a name held in a string"),
    ("vars", "vars() hands the model over as a mapping"),
];

/// Attributes that hand a model over as a mapping.
const MAPPING_ATTRIBUTES: &[&str] = &["__dict__", "model_extra", "__pydantic_extra__"];

/// Whether a call passes the keyword argument of that name.
fn has_keyword_argument(call: tree_sitter::Node, keyword: &str, source: &[u8]) -> bool {
    let Some(arguments) = node_field(call, "arguments") else {
        return false;
    };
    let mut cursor = arguments.walk();
    let found = arguments.named_children(&mut cursor).any(|argument| {
        argument.kind() == "keyword_argument"
            && node_field(argument, "name").is_some_and(|name| node_text(name, source) == keyword)
    });
    found
}

/// What a builtin call does to a model without validating it, if anything.
fn builtin_bypass(name: &str) -> Option<&'static str> {
    BUILTIN_BYPASSES.iter().find(|(builtin, _)| *builtin == name).map(|(_, finding)| *finding)
}

/// What a method call does to a model without validating it, if anything.
fn method_bypass(call: tree_sitter::Node, method: &str, source: &[u8]) -> Option<&'static str> {
    match method {
        "model_construct" => Some("model_construct() builds a model without validating it"),
        "model_copy" if has_keyword_argument(call, "update", source) => {
            Some("model_copy(update=...) sets fields without validating them")
        }
        _ => None,
    }
}

/// What a call does to a model without validating it, if anything.
fn call_bypass(call: tree_sitter::Node, source: &[u8]) -> Option<&'static str> {
    let function = node_field(call, "function")?;
    match function.kind() {
        "identifier" => builtin_bypass(node_text(function, source)),
        "attribute" => method_bypass(call, node_text(node_field(function, "attribute")?, source), source),
        _ => None,
    }
}

/// Detect a model read or changed as a bag of keys: `model_copy(update=...)`,
/// `model_construct`, `getattr`/`setattr`/`delattr`, `vars()`, `.__dict__` and
/// the extras mapping. None of them validates, so the model's types hold no
/// longer than the call.
pub fn check_no_model_bypass(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let bytes = source.source_bytes;
    let root = source.tree.root_node();
    let mut violations: Vec<Violation> = find_nodes_by_type(root, "call")
        .into_iter()
        .filter_map(|call| call_bypass(call, bytes).map(|finding| violation(node_line(call), finding.to_string())))
        .collect();

    for attribute in find_nodes_by_type(root, "attribute") {
        let Some(name_node) = node_field(attribute, "attribute") else {
            continue;
        };
        let name = node_text(name_node, bytes);
        if MAPPING_ATTRIBUTES.contains(&name) {
            violations.push(violation(node_line(attribute), format!(".{name} hands the model over as a mapping")));
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

    let content = std::str::from_utf8(source.source_bytes).unwrap_or("");
    if content.trim().is_empty() {
        return Vec::new();
    }

    vec![violation(1, "non-empty __init__.py".to_string())]
}

// -------------------------------------------------------------------------
// no_nested_functions — any function_definition inside another
// -------------------------------------------------------------------------

/// Every function_definition inside another, each tagged with the check it
/// belongs to: `no_closures` when the inner function references names from the
/// outer scope, `no_nested_functions` when it does not.
fn nested_function_violations(source: &ParsedSource) -> Vec<Violation> {
    let mut violations = Vec::new();
    let root = source.tree.root_node();
    collect_nested_functions(root, None, source.source_bytes, &mut violations);
    violations
}

fn nested_function_violations_of(source: &ParsedSource, check: &CheckMessages) -> Vec<Violation> {
    nested_function_violations(source)
        .into_iter()
        .filter(|found| found.check_name == check.name)
        .collect()
}

/// Detect nested function definitions that capture nothing from the enclosing
/// function. A nested function that does capture is `check_no_closures`'s.
pub fn check_no_nested_functions(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    nested_function_violations_of(source, &messages::NO_NESTED_FUNCTIONS)
}

/// Detect closures: nested functions that reference names from the enclosing
/// function's scope.
pub fn check_no_closures(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    nested_function_violations_of(source, &messages::NO_CLOSURES)
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
                violations.push(messages::NO_CLOSURES.violation(
                    node_line(node),
                    format!("closure '{inner_name}' captures from enclosing scope"),
                ));
            } else {
                violations.push(messages::NO_NESTED_FUNCTIONS.violation(
                    node_line(node),
                    format!("nested function '{inner_name}'"),
                ));
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
        .map(|node| {
            messages::NO_GENERAL_LAMBDA
                .violation(node_line(*node), "lambda expression — extract to a named function".to_string())
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
        .map(|node| {
            messages::NO_DEFAULT_FACTORY.violation(
                node_line(*node),
                "default_factory=lambda — use a named function or class default".to_string(),
            )
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
    let root = source.tree.root_node();
    let partial_names = names_for(root, source.source_bytes, "partial", &["functools"]);
    let mut violations = Vec::new();
    for node in find_nodes_by_type(root, "call") {
        if calls_named(node, "partial", &partial_names, source.source_bytes) {
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
    let root = source.tree.root_node();
    let bytes = source.source_bytes;
    let mut re_names = module_names(root, bytes, "re");
    re_names.push("re".to_string());
    let sub_names = bound_names(root, bytes, "re", "sub");
    let compile_names = bound_names(root, bytes, "re", "compile");
    let patterns = compiled_pattern_names(root, &re_names, &compile_names, bytes);
    let mut violations = Vec::new();
    for node in find_nodes_by_type(root, "call") {
        if is_re_sub_call(node, &re_names, &sub_names, bytes) || is_pattern_sub_call(node, &re_names, &compile_names, &patterns, bytes) {
            violations.push(violation(
                node_line(node),
                "re.sub() — use re.finditer() with a loop instead".to_string(),
            ));
        }
    }
    violations
}

/// Whether a call is `re.compile(...)`, through a name the file uses for `re`
/// or for `compile` imported from it.
fn is_compile_call(call: tree_sitter::Node, re_names: &[String], compile_names: &[String], source: &[u8]) -> bool {
    let Some(func) = call.child_by_field_name("function") else {
        return false;
    };
    let named = |node: tree_sitter::Node, names: &[String]| names.iter().any(|name| name == node_text(node, source));
    match func.kind() {
        "identifier" => named(func, compile_names),
        "attribute" => {
            func.child_by_field_name("object").is_some_and(|object| named(object, re_names))
                && func.child_by_field_name("attribute").is_some_and(|attribute| node_text(attribute, source) == "compile")
        }
        _ => false,
    }
}

/// The names a file binds to a compiled pattern: `PATTERN = re.compile(...)`.
fn compiled_pattern_names(
    root: tree_sitter::Node,
    re_names: &[String],
    compile_names: &[String],
    source: &[u8],
) -> Vec<String> {
    find_nodes_by_type(root, "assignment")
        .into_iter()
        .filter(|assignment| {
            assignment
                .child_by_field_name("right")
                .is_some_and(|right| right.kind() == "call" && is_compile_call(right, re_names, compile_names, source))
        })
        .filter_map(|assignment| assignment.child_by_field_name("left").filter(|left| left.kind() == "identifier"))
        .map(|left| node_text(left, source).to_string())
        .collect()
}

/// Whether a call is `.sub(...)` on a compiled pattern: one a name holds, or
/// `re.compile(...)` itself.
fn is_pattern_sub_call(
    call: tree_sitter::Node,
    re_names: &[String],
    compile_names: &[String],
    patterns: &[String],
    source: &[u8],
) -> bool {
    let Some(func) = call.child_by_field_name("function").filter(|func| func.kind() == "attribute") else {
        return false;
    };
    if !func.child_by_field_name("attribute").is_some_and(|attribute| node_text(attribute, source) == "sub") {
        return false;
    }
    func.child_by_field_name("object").is_some_and(|object| match object.kind() {
        "identifier" => patterns.iter().any(|pattern| pattern == node_text(object, source)),
        "call" => is_compile_call(object, re_names, compile_names, source),
        _ => false,
    })
}

/// Whether a call is `re.sub`: through a name the file uses for `re`, or
/// through a name it imported `sub` from `re` under.
fn is_re_sub_call(call: tree_sitter::Node, module_names: &[String], sub_names: &[String], source: &[u8]) -> bool {
    let Some(func) = call.child_by_field_name("function") else {
        return false;
    };
    let named = |node: tree_sitter::Node, names: &[String]| names.iter().any(|name| name == node_text(node, source));
    match func.kind() {
        "identifier" => named(func, sub_names),
        "attribute" => {
            let object = func.child_by_field_name("object");
            let attribute = func.child_by_field_name("attribute");
            object.is_some_and(|object| named(object, module_names))
                && attribute.is_some_and(|attribute| node_text(attribute, source) == "sub")
        }
        _ => false,
    }
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
    fn cast_under_another_name_caught() {
        let parsed = parse("from typing import cast as trust\nx = trust(int, value)\n");
        assert_eq!(check_no_cast(&parsed, &default_config()).len(), 1);
    }

    #[test]
    fn a_function_that_is_not_cast_is_not_cast() {
        let parsed = parse("from mypkg.shapes import trust\nx = trust(int, value)\n");
        assert!(check_no_cast(&parsed, &default_config()).is_empty());
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
    fn broad_exception_in_a_tuple_or_as_base_exception_caught() {
        // The canary: the broad type hidden beside a specific one, or one level up.
        for (shown, code) in [
            ("Exception", "try:\n    pass\nexcept (ValueError, Exception):\n    pass\n"),
            ("Exception", "try:\n    pass\nexcept (KeyError, builtins.Exception) as caught:\n    pass\n"),
            ("BaseException", "try:\n    pass\nexcept BaseException:\n    pass\n"),
            ("Exception", "try:\n    pass\nexcept Exception as caught:\n    pass\n"),
        ] {
            let violations = check_no_broad_exceptions(&parse(code), &default_config());
            assert_eq!(violations.len(), 1, "{code}");
            assert_eq!(violations[0].message, format!("except {shown} is too broad"));
        }
    }

    #[test]
    fn specific_exceptions_in_a_tuple_ok() {
        let parsed = parse("try:\n    pass\nexcept (ValueError, KeyError) as caught:\n    pass\n");
        assert!(check_no_broad_exceptions(&parsed, &default_config()).is_empty());
    }

    #[test]
    fn init_with_any_content_caught() {
        let parsed = parse_init("# This __init__.py file is EPHEMERAL: generated\nfrom foo import bar\n");
        assert_eq!(check_init_files_empty(&parsed, &default_config()).len(), 1);
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

    #[test]
    fn model_dump_with_arguments_caught() {
        for code in [
            "result = obj.model_dump(mode=\"json\")\n",
            "result = obj.model_dump(exclude_none=True, by_alias=True)\n",
            "result = obj.dict(exclude_none=True)\n",
        ] {
            let violations = check_no_model_dump(&parse(code), &default_config());
            assert_eq!(violations.len(), 1, "{code}");
        }
    }

    #[test]
    fn model_dump_json_ok() {
        for code in [
            "text = obj.model_dump_json()\n",
            "text = obj.model_dump_json(by_alias=True, exclude_none=True)\n",
        ] {
            let violations = check_no_model_dump(&parse(code), &default_config());
            assert!(violations.is_empty(), "{code}");
        }
    }

    // -- no_model_bypass --

    fn bypasses(code: &str) -> Vec<Violation> {
        check_no_model_bypass(&parse(code), &default_config())
    }

    #[test]
    fn model_copy_with_update_caught() {
        // The specimen: kind lines set on a model with no fields, from a plain dict.
        let found = bypasses("frontmatter = document.frontmatter.model_copy(update=lines_to_set(reclassification))\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message, "model_copy(update=...) sets fields without validating them");
    }

    #[test]
    fn model_copy_without_update_ok() {
        for code in ["copied = page.model_copy()\n", "copied = page.model_copy(deep=True)\n"] {
            assert!(bypasses(code).is_empty(), "{code}");
        }
    }

    #[test]
    fn model_construct_caught() {
        let found = bypasses("page = Page.model_construct(title=title)\n");
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("model_construct()"));
    }

    #[test]
    fn a_field_reached_by_a_name_in_a_string_caught() {
        for (builtin, code) in [
            ("getattr", "value = getattr(page, \"title\")\n"),
            ("getattr", "value = getattr(page, field_name, None)\n"),
            ("setattr", "setattr(page, \"title\", title)\n"),
            ("delattr", "delattr(page, field_name)\n"),
        ] {
            let found = bypasses(code);
            assert_eq!(found.len(), 1, "{code}");
            assert_eq!(found[0].message, format!("{builtin}() reaches a field by a name held in a string"));
        }
    }

    #[test]
    fn a_model_handed_over_as_a_mapping_caught() {
        for (shown, code) in [
            ("vars()", "fields = vars(page)\n"),
            (".__dict__", "fields = page.__dict__\n"),
            (".model_extra", "extras = page.model_extra\n"),
            (".__pydantic_extra__", "extras = page.__pydantic_extra__\n"),
        ] {
            let found = bypasses(code);
            assert_eq!(found.len(), 1, "{code}");
            assert_eq!(found[0].message, format!("{shown} hands the model over as a mapping"));
        }
    }

    #[test]
    fn a_model_built_and_read_by_its_fields_ok() {
        let code = "page = Page(title=other.title, tags=other.tags)\ntitle = page.title\ncopied = page.copy_of\nmonkeypatch.setattr(target, \"name\", value)\nhint = \"use getattr(page, name)\"\n";
        assert!(bypasses(code).is_empty());
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
        let violations = check_no_closures(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].check_name, "no_closures");
        assert!(violations[0].message.contains("closure"));
        assert!(
            check_no_nested_functions(&parsed, &default_config()).is_empty(),
            "a closure belongs to no_closures alone"
        );
    }

    #[test]
    fn nested_function_that_captures_nothing_is_not_a_closure() {
        let code = "def outer():\n    def inner():\n        return 1\n    return inner()\n";
        let parsed = parse(code);
        assert!(check_no_closures(&parsed, &default_config()).is_empty());
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
    fn partial_under_another_name_caught() {
        let parsed = parse("from functools import partial as bind\ncallback = bind(resolve, my_values)\n");
        assert_eq!(check_no_partial(&parsed, &default_config()).len(), 1);
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
    fn re_sub_under_another_name_caught() {
        for code in [
            "from re import sub\nresult = sub(r'pattern', callback_fn, text)\n",
            "from re import sub as replace\nresult = replace(r'pattern', callback_fn, text)\n",
            "import re as regex\nresult = regex.sub(r'pattern', callback_fn, text)\n",
        ] {
            assert_eq!(check_no_re_sub(&parse(code), &default_config()).len(), 1, "{code}");
        }
    }

    #[test]
    fn sub_on_a_compiled_pattern_caught() {
        // The canary: the callback API reached through the pattern instead of the module.
        for code in [
            "import re\nPATTERN = re.compile(r'x')\nresult = PATTERN.sub(callback_fn, text)\n",
            "import re\nresult = re.compile(r'x').sub(callback_fn, text)\n",
            "from re import compile as build\nword = build(r'x')\nresult = word.sub(callback_fn, text)\n",
        ] {
            assert_eq!(check_no_re_sub(&parse(code), &default_config()).len(), 1, "{code}");
        }
    }

    #[test]
    fn sub_on_something_not_compiled_by_re_ok() {
        let code = "frame = load()\nresult = frame.sub(other)\nPATTERN = build(r'x')\nresult = PATTERN.sub(other)\n";
        assert!(check_no_re_sub(&parse(code), &default_config()).is_empty());
    }

    #[test]
    fn a_sub_that_is_not_re_sub_ok() {
        for code in ["result = sub(first, second)\n", "import operator\nresult = operator.sub(first, second)\n"] {
            assert!(check_no_re_sub(&parse(code), &default_config()).is_empty(), "{code}");
        }
    }

    #[test]
    fn re_finditer_ok() {
        let code = "import re\nfor match in re.finditer(r'pattern', text):\n    pass\n";
        let parsed = parse(code);
        let violations = check_no_re_sub(&parsed, &default_config());
        assert!(violations.is_empty());
    }

}
