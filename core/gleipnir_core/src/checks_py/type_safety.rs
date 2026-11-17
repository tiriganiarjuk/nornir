//! Type system enforcement checks.
//!
//! Checks: no_object, no_json_value, no_any_types, no_any_type_aliases,
//! no_bare_collections, union_member_count, no_callable_params,
//! no_implicit_type_aliases.

use crate::parsing::{
    annotation_contains_name, bare_names_in_annotation, collect_union_leaves,
    count_union_members, find_nodes_by_type, find_type_annotations, node_field, node_line,
    node_text, walk_tree,
};
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

pub fn check_no_object(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for type_node in find_type_annotations(source.tree.root_node(), source.source_bytes) {
        if annotation_contains_name(type_node, "object", source.source_bytes) {
            let text = node_text(type_node, source.source_bytes);
            violations.push(violation(
                node_line(type_node),
                format!("object in type annotation: {text}"),
            ));
        }
    }
    violations
}

pub fn check_no_json_value(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for type_node in find_type_annotations(source.tree.root_node(), source.source_bytes) {
        if annotation_contains_name(type_node, "JsonValue", source.source_bytes) {
            let text = node_text(type_node, source.source_bytes);
            violations.push(violation(
                node_line(type_node),
                format!("JsonValue in type annotation: {text}"),
            ));
        }
    }
    violations
}

pub fn check_no_any_types(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {

    let mut violations = Vec::new();

    for type_node in find_type_annotations(source.tree.root_node(), source.source_bytes) {
        if annotation_contains_name(type_node, "Any", source.source_bytes) {
            let text = node_text(type_node, source.source_bytes);
            violations.push(violation(
                node_line(type_node),
                format!("Any in type annotation: {text}"),
            ));
        }
    }
    violations
}

static BARE_COLLECTION_NAMES: &[&str] = &["dict", "list", "set", "tuple"];

pub fn check_no_bare_collections(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for type_node in find_type_annotations(source.tree.root_node(), source.source_bytes) {
        for (line, name) in
            bare_names_in_annotation(type_node, BARE_COLLECTION_NAMES, source.source_bytes)
        {
            violations.push(violation(line, format!("bare {name} in type annotation")));
        }
    }
    violations
}

/// Python's builtin type names: the one list for both checks that ask whether a
/// name is a builtin type. `union_member_count` counts a union member with one of
/// these names as "simple", bare or parameterized (list[int], dict[str, float]);
/// `no_implicit_type_aliases` takes one as a type-shaped operand of a `|` alias.
const BUILTIN_TYPE_NAMES: &[&str] = &[
    "int", "str", "float", "bool", "bytes", "complex", "bytearray", "memoryview",
    "list", "dict", "tuple", "set", "frozenset", "object", "type",
];

/// Classify whether a union leaf node is a simple (builtin) type.
///
/// Returns true for bare builtins (`int`) and parameterized builtins (`list[int]`).
/// None is handled separately by the caller, not here.
fn is_simple_type(node: tree_sitter::Node, source: &[u8]) -> bool {
    match node.kind() {
        "identifier" => {
            let text = node_text(node, source);
            BUILTIN_TYPE_NAMES.contains(&text.as_ref())
        }
        // tree-sitter uses "generic_type" for parameterized types in annotations (list[int])
        // and "subscript" in expression context
        "generic_type" | "subscript" => {
            // Check if the base identifier is a simple/builtin name
            // generic_type: first named child is the identifier
            // subscript: "value" field is the identifier
            let base = node.named_child(0)
                .or_else(|| node.child_by_field_name("value"));
            if let Some(base_node) = base {
                if base_node.kind() == "identifier" {
                    let text = node_text(base_node, source);
                    return BUILTIN_TYPE_NAMES.contains(&text.as_ref());
                }
            }
            false
        }
        _ => false,
    }
}

/// Check if a union leaf node is None.
fn is_none_type(node: tree_sitter::Node, source: &[u8]) -> bool {
    node.kind() == "none" || (node.kind() == "identifier" && node_text(node, source) == "None")
}

/// Check if a node is the root of a union (binary_operator with | OR union_type).
fn is_union_root(node: tree_sitter::Node) -> bool {
    match node.kind() {
        "union_type" => {
            // Skip nested union_type (shouldn't happen but be safe)
            if let Some(parent) = node.parent() {
                if parent.kind() == "union_type" {
                    return false;
                }
            }
            true
        }
        "binary_operator" => {
            // Skip if parent is also binary_operator (not the outermost)
            if let Some(parent) = node.parent() {
                if parent.kind() == "binary_operator" {
                    return false;
                }
            }
            count_union_members(node) > 1
        }
        _ => false,
    }
}

/// Find top-level union nodes within an annotation subtree.
/// Returns (line, simple_count, named_count) for each union root.
///
/// Handles both binary_operator chains (simple type unions) and
/// union_type nodes (tree-sitter uses this for generic/parameterized unions).
fn find_union_roots(
    type_node: tree_sitter::Node,
    source: &[u8],
) -> Vec<(usize, usize, usize)> {
    let mut results = Vec::new();

    for node in walk_tree(type_node) {
        if !is_union_root(node) {
            continue;
        }
        let leaves = collect_union_leaves(node);
        let mut simple_count = 0usize;
        let mut named_count = 0usize;
        for leaf in &leaves {
            if is_none_type(*leaf, source) {
                continue; // None is always free
            } else if is_simple_type(*leaf, source) {
                simple_count += 1;
            } else {
                named_count += 1;
            }
        }
        if simple_count + named_count > 1 {
            results.push((node_line(node), simple_count, named_count));
        }
    }
    results
}

pub fn check_union_member_count(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for type_node in find_type_annotations(source.tree.root_node(), source.source_bytes) {
        for (line, simple_count, named_count) in find_union_roots(type_node, source.source_bytes) {
            if simple_count > config.max_simple_union_members {
                violations.push(violation(
                    line,
                    "union has too many simple types".to_string(),
                ));
            }
            if named_count > config.max_named_union_members {
                violations.push(violation(
                    line,
                    "union has too many named types".to_string(),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_callable_params
// -------------------------------------------------------------------------

/// Detect Callable in function parameter type annotations.
///
/// Callable parameters launder runtime dependencies through function arguments,
/// bypassing the static import graph that gleipnir enforces. If a function needs
/// to call another function, it must import it directly.
pub fn check_no_callable_params(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let params = match node_field(func, "parameters") {
            Some(p) => p,
            None => continue,
        };
        let mut cursor = params.walk();
        for param in params.named_children(&mut cursor) {
            // typed_parameter, typed_default_parameter
            let type_node = node_field(param, "type");
            let type_node = match type_node {
                Some(t) => t,
                None => continue,
            };
            if annotation_contains_name(type_node, "Callable", source.source_bytes) {
                let param_name = node_field(param, "name")
                    .map(|n| node_text(n, source.source_bytes))
                    .unwrap_or_else(|| node_text(param, source.source_bytes));
                violations.push(violation(
                    node_line(param),
                    format!("parameter '{param_name}' has Callable type annotation"),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_callable_type_aliases
// -------------------------------------------------------------------------

/// Detect type aliases that wrap Callable, laundering callable-passing behind
/// a clean name that `no_callable_params` cannot see through.
pub fn check_no_callable_type_aliases(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "type_alias_statement") {
        let mut cursor = node.walk();
        let type_children: Vec<_> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "type")
            .collect();

        if type_children.len() < 2 {
            continue;
        }

        let name_node = type_children[0];
        let value_node = match type_children.last() {
            Some(node) => *node,
            None => continue,
        };

        let alias_name = name_node
            .named_child(0)
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("<unknown>");

        if annotation_contains_name(value_node, "Callable", source.source_bytes) {
            violations.push(violation(
                node_line(node),
                format!(
                    "type alias '{alias_name}' wraps Callable — callables must not be passed as arguments"
                ),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_any_type_aliases
// -------------------------------------------------------------------------

pub fn check_no_any_type_aliases(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "type_alias_statement") {
        // Extract alias name (second named child of kind "type" wrapping an identifier)
        // and value (last named child of kind "type")
        let mut cursor = node.walk();
        let type_children: Vec<_> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "type")
            .collect();

        // Need at least 2: name wrapper and value wrapper
        if type_children.len() < 2 {
            continue;
        }

        let name_node = type_children[0];
        let value_node = match type_children.last() {
            Some(node) => *node,
            None => continue,
        };

        let alias_name = name_node
            .named_child(0)
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("<unknown>");

        if annotation_contains_name(value_node, "Any", source.source_bytes) {
            violations.push(violation(
                node_line(node),
                format!("type alias '{alias_name}' launders Any — use Any directly so type holes cluster visibly"),
            ));
        }
    }
    violations
}

/// Classify a node as a type alias value.
/// Returns alias type name or empty string.
fn is_type_alias_value(node: tree_sitter::Node, source: &[u8]) -> &'static str {
    if node.kind() == "subscript" {
        if let Some(value) = node.child_by_field_name("value") {
            if value.kind() == "identifier" {
                match node_text(value, source) {
                    "Literal" => return "Literal",
                    "dict" => return "dict",
                    "list" => return "list",
                    "set" => return "set",
                    "tuple" => return "tuple",
                    _ => {}
                }
            }
        }
    }
    if is_type_union(node, source) {
        return "Union";
    }
    ""
}

/// CapWords: starts uppercase and contains at least one lowercase letter.
fn is_cap_words(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
        && name.contains(|c: char| c.is_ascii_lowercase())
}

/// Check if a binary_operator node's operator is `|`.
fn has_pipe_operator(node: tree_sitter::Node) -> bool {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .any(|child| !child.is_named() && child.kind() == "|");
    found
}

/// Check if an operand of `|` names a type: None, a builtin type, a subscript
/// (`list[int]`), a CapWords name, an attribute ending in CapWords, or a
/// nested `|` of such operands.
fn is_type_shaped_operand(node: tree_sitter::Node, source: &[u8]) -> bool {
    match node.kind() {
        "none" | "subscript" => true,
        "identifier" => {
            let name = node_text(node, source);
            name == "None" || BUILTIN_TYPE_NAMES.contains(&name) || is_cap_words(name)
        }
        "attribute" => node
            .child_by_field_name("attribute")
            .map(|segment| is_cap_words(node_text(segment, source)))
            .unwrap_or(false),
        "binary_operator" => is_type_union(node, source),
        _ => false,
    }
}

/// Check if a node is a `|` expression whose every operand is type-shaped.
/// A `|` between values (sets, dicts, flags, constants) is an operation, not an alias.
fn is_type_union(node: tree_sitter::Node, source: &[u8]) -> bool {
    if node.kind() != "binary_operator" || !has_pipe_operator(node) {
        return false;
    }
    let left = node.child_by_field_name("left");
    let right = node.child_by_field_name("right");
    match (left, right) {
        (Some(left_operand), Some(right_operand)) => {
            is_type_shaped_operand(left_operand, source)
                && is_type_shaped_operand(right_operand, source)
        }
        _ => false,
    }
}

pub fn check_no_implicit_type_aliases(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let root = source.tree.root_node();
    let mut cursor = root.walk();

    for stmt in root.named_children(&mut cursor) {
        let assign = if stmt.kind() == "expression_statement" {
            let mut inner_cursor = stmt.walk();
            let first_child = stmt.named_children(&mut inner_cursor).next();
            match first_child {
                Some(child) if child.kind() == "assignment" => Some(child),
                _ => None,
            }
        } else if stmt.kind() == "assignment" {
            Some(stmt)
        } else {
            None
        };

        let assign = match assign {
            Some(a) => a,
            None => continue,
        };

        let left = match node_field(assign, "left") {
            Some(n) if n.kind() == "identifier" => n,
            _ => continue,
        };
        let right = match node_field(assign, "right") {
            Some(n) => n,
            None => continue,
        };

        let alias_type = is_type_alias_value(right, source.source_bytes);
        if !alias_type.is_empty() {
            let name = node_text(left, source.source_bytes);
            violations.push(violation(
                node_line(assign),
                format!("implicit type alias {name} = {alias_type}[...]"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_string_annotations — string forward references in type positions
// -------------------------------------------------------------------------

/// Check if a type annotation node is a string literal (forward reference).
///
/// Tree-sitter wraps return types in a `type` node, so we check both
/// the node itself and its first named child.
fn is_string_type_node<'a>(node: tree_sitter::Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    if node.kind() == "string" {
        return Some(node_text(node, source));
    }
    // return_type wraps in a "type" node
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "string" {
            return Some(node_text(child, source));
        }
    }
    None
}

/// Detect string annotations used as forward references.
///
/// `-> "SomeType"` or `param: "SomeType"` — the type is a string literal
/// instead of an actual type reference. This hides the real dependency
/// and exists because the import was deferred or missing.
pub fn check_no_string_annotations(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    // Return type annotations: def foo() -> "Bar"
    for func in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        if let Some(ret) = node_field(func, "return_type") {
            if let Some(text) = is_string_type_node(ret, source.source_bytes) {
                violations.push(violation(
                    node_line(func),
                    format!("string annotation {text} — use a real type reference"),
                ));
            }
        }
    }

    // Parameter type annotations: def foo(x: "Bar")
    for kind in &["typed_parameter", "typed_default_parameter"] {
        for param in find_nodes_by_type(source.tree.root_node(), kind) {
            if let Some(type_node) = node_field(param, "type") {
                if let Some(text) = is_string_type_node(type_node, source.source_bytes) {
                    violations.push(violation(
                        node_line(param),
                        format!("string annotation {text} — use a real type reference"),
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
    use crate::parsing::build_parsed_source;

    fn parse(code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source("/test/file.py", source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(crate::structures::FileKind::Outside, &crate::STATISTICS)
    }

    // -- no_object --

    #[test]
    fn object_in_annotation() {
        let parsed = parse("def foo(x: object) -> None:\n    pass\n");
        let violations = check_no_object(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("object"));
    }

    #[test]
    fn no_object_clean() {
        let parsed = parse("def foo(x: str) -> str:\n    return x\n");
        let violations = check_no_object(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_json_value --

    #[test]
    fn json_value_in_annotation() {
        let parsed = parse("x: JsonValue = {}\n");
        let violations = check_no_json_value(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("JsonValue"));
    }

    #[test]
    fn no_json_value_clean() {
        let parsed = parse("x: dict[str, int] = {}\n");
        let violations = check_no_json_value(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_any_types --

    #[test]
    fn any_in_annotation() {
        let parsed = parse("def foo(x: Any) -> Any:\n    return x\n");
        let violations = check_no_any_types(&parsed, &default_config());
        assert!(!violations.is_empty());
    }

    #[test]
    fn no_any_clean() {
        let parsed = parse("def foo(x: str) -> int:\n    return 1\n");
        let violations = check_no_any_types(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn any_in_type_alias_caught() {
        let parsed = parse("type JsonNode = dict[str, Any]\n");
        let violations = check_no_any_types(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Any"));
    }

    // -- no_bare_collections --

    #[test]
    fn bare_dict_caught() {
        let parsed = parse("x: dict\n");
        let violations = check_no_bare_collections(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("dict"));
    }

    #[test]
    fn subscripted_dict_ok() {
        let parsed = parse("x: dict[str, int]\n");
        let violations = check_no_bare_collections(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn bare_list_caught() {
        let parsed = parse("def foo(items: list) -> None:\n    pass\n");
        let violations = check_no_bare_collections(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("list"));
    }

    // -- union_member_count --

    #[test]
    fn small_simple_union_ok() {
        let parsed = parse("x: int | str\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 2 simple, cap 4
    }

    #[test]
    fn simple_at_cap_ok() {
        let parsed = parse("x: int | str | float | bool\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 4 simple, cap 4
    }

    #[test]
    fn simple_over_cap_caught() {
        let parsed = parse("x: int | str | float | bool | bytes\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1); // 5 simple, cap 4
        assert!(violations[0].message.contains("simple types"));
    }

    #[test]
    fn none_not_counted() {
        let parsed = parse("x: int | str | float | bool | None\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 4 simple + None free = ok
    }

    #[test]
    fn named_at_cap_ok() {
        let parsed = parse(
            "x: ModelA | ModelB | ModelC | ModelD | ModelE | ModelF | ModelG | ModelH\n",
        );
        let violations = check_union_member_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 8 named, cap 8
    }

    #[test]
    fn named_over_cap_caught() {
        let parsed = parse(
            "x: ModelA | ModelB | ModelC | ModelD | ModelE | ModelF | ModelG | ModelH | ModelI\n",
        );
        let violations = check_union_member_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1); // 9 named, cap 8
        assert!(violations[0].message.contains("named types"));
    }

    #[test]
    fn named_with_none_ok() {
        let parsed = parse(
            "x: ModelA | ModelB | ModelC | ModelD | ModelE | ModelF | ModelG | ModelH | None\n",
        );
        let violations = check_union_member_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 8 named + None free
    }

    #[test]
    fn parameterized_builtin_is_simple() {
        let parsed = parse("x: list[int] | dict[str, float] | tuple[bool, ...] | set[bytes] | frozenset[int]\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1); // 5 simple (parameterized), cap 4
        assert!(violations[0].message.contains("simple types"));
    }

    #[test]
    fn mixed_both_under_caps_ok() {
        let parsed = parse("x: int | str | ModelA | ModelB | ModelC | ModelD | ModelE | ModelF\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 2 simple (cap 4), 6 named (cap 8)
    }

    #[test]
    fn mixed_simple_over_cap() {
        let parsed = parse(
            "x: int | str | float | bool | bytes | ModelA | ModelB\n",
        );
        let violations = check_union_member_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1); // 5 simple over cap
        assert!(violations[0].message.contains("simple types"));
    }

    /// The four builtin type names that pin the one list in both of its uses.
    const PINNED_BUILTIN_TYPE_NAMES: [&str; 4] = ["type", "complex", "bytearray", "memoryview"];

    #[test]
    fn each_pinned_builtin_name_is_a_simple_union_member() {
        for name in PINNED_BUILTIN_TYPE_NAMES {
            let parsed = parse(&format!("x: int | str | float | bool | {name}\n"));
            let violations = check_union_member_count(&parsed, &default_config());
            assert_eq!(violations.len(), 1, "{name}"); // 5 simple, cap 4
            assert!(violations[0].message.contains("simple types"), "{name}");
        }
    }

    #[test]
    fn a_name_that_only_resembles_a_pinned_builtin_is_a_named_union_member() {
        for name in PINNED_BUILTIN_TYPE_NAMES {
            let parsed = parse(&format!("x: int | str | float | bool | {name}_like\n"));
            let violations = check_union_member_count(&parsed, &default_config());
            assert!(violations.is_empty(), "{name}_like"); // 4 simple, 1 named
        }
    }

    #[test]
    fn union_member_count_messages_carry_no_numbers() {
        let simple = parse("x: int | str | float | bool | bytes\n");
        let named = parse(
            "x: ModelA | ModelB | ModelC | ModelD | ModelE | ModelF | ModelG | ModelH | ModelI\n",
        );
        let mut violations = check_union_member_count(&simple, &default_config());
        violations.extend(check_union_member_count(&named, &default_config()));
        assert_eq!(violations.len(), 2);
        for viol in &violations {
            assert!(!viol.message.contains(|c: char| c.is_ascii_digit()), "{}", viol.message);
        }
    }

    // -- no_callable_params --

    #[test]
    fn callable_param_caught() {
        let parsed = parse("def run(fn: Callable[[int], str]) -> None:\n    pass\n");
        let violations = check_no_callable_params(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Callable"));
    }

    #[test]
    fn bare_callable_param_caught() {
        let parsed = parse("def run(fn: Callable) -> None:\n    pass\n");
        let violations = check_no_callable_params(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn callable_in_union_param_caught() {
        let parsed = parse("def run(fn: Callable | None) -> None:\n    pass\n");
        let violations = check_no_callable_params(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn no_callable_param_ok() {
        let parsed = parse("def run(x: int, y: str) -> None:\n    pass\n");
        let violations = check_no_callable_params(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn callable_return_type_ok() {
        let parsed = parse("def factory() -> Callable[[int], str]:\n    pass\n");
        let violations = check_no_callable_params(&parsed, &default_config());
        assert!(violations.is_empty()); // return type, not param
    }

    // -- no_implicit_type_aliases --

    #[test]
    fn literal_alias_caught() {
        let parsed = parse("Status = Literal['active', 'inactive']\n");
        let violations = check_no_implicit_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Literal"));
    }

    #[test]
    fn union_alias_caught() {
        let parsed = parse("Result = int | str\n");
        let violations = check_no_implicit_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Union"));
    }

    #[test]
    fn union_alias_of_type_shaped_operands_caught() {
        for code in [
            "Maybe = Foo | None\n",
            "Handler = list[int] | Mapping\n",
            "Number = int | float | Decimal\n",
            "Shape = shapes.Circle | shapes.Square\n",
        ] {
            let parsed = parse(code);
            let violations = check_no_implicit_type_aliases(&parsed, &default_config());
            assert_eq!(violations.len(), 1, "{code}");
            assert!(violations[0].message.contains("Union"), "{code}");
        }
    }

    #[test]
    fn each_pinned_builtin_name_is_a_type_shaped_alias_operand() {
        for name in PINNED_BUILTIN_TYPE_NAMES {
            let parsed = parse(&format!("Maybe = {name} | None\n"));
            let violations = check_no_implicit_type_aliases(&parsed, &default_config());
            assert_eq!(violations.len(), 1, "{name}");
            assert!(violations[0].message.contains("Union"), "{name}");
        }
    }

    #[test]
    fn a_name_that_only_resembles_a_pinned_builtin_is_not_a_type_shaped_alias_operand() {
        for name in PINNED_BUILTIN_TYPE_NAMES {
            let parsed = parse(&format!("merged = {name}_like | None\n"));
            let violations = check_no_implicit_type_aliases(&parsed, &default_config());
            assert!(violations.is_empty(), "{name}_like");
        }
    }

    #[test]
    fn pipe_between_values_is_not_an_alias() {
        for code in [
            "ALL = FIRST_SET | SECOND_SET\n",
            "merged = defaults | overrides\n",
            "FLAGS = re.I | re.M\n",
            "X = {1} | {2}\n",
            "Combined = build() | Foo\n",
            "Mask = Foo | 4\n",
            "Mixed = int | str | SOME_CONSTANT\n",
            "Total = Foo + Bar\n",
        ] {
            let parsed = parse(code);
            let violations = check_no_implicit_type_aliases(&parsed, &default_config());
            assert!(violations.is_empty(), "{code}");
        }
    }

    #[test]
    fn normal_assignment_ok() {
        let parsed = parse("x = 42\n");
        let violations = check_no_implicit_type_aliases(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn dict_alias_caught() {
        let parsed = parse("Config = dict[str, int]\n");
        let violations = check_no_implicit_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- no_any_type_aliases --

    #[test]
    fn any_type_alias_caught() {
        let parsed = parse("type JsonNode = dict[str, Any]\n");
        let violations = check_no_any_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("JsonNode"));
        assert!(violations[0].message.contains("launders Any"));
    }

    #[test]
    fn nested_any_type_alias_caught() {
        let parsed = parse("type Payload = list[dict[str, Any]]\n");
        let violations = check_no_any_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Payload"));
    }

    #[test]
    fn clean_type_alias_ok() {
        let parsed = parse("type UserId = str\n");
        let violations = check_no_any_type_aliases(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn type_alias_no_any_ok() {
        let parsed = parse("type Config = dict[str, int]\n");
        let violations = check_no_any_type_aliases(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_string_annotations --

    #[test]
    fn string_return_annotation_caught() {
        let parsed = parse("def assemble() -> \"SectionBuffer\":\n    pass\n");
        let violations = check_no_string_annotations(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("string annotation"));
    }

    #[test]
    fn string_param_annotation_caught() {
        let parsed = parse("def process(data: \"InputModel\") -> None:\n    pass\n");
        let violations = check_no_string_annotations(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn real_type_annotations_ok() {
        let parsed = parse("def add(a: int, b: int) -> int:\n    return a + b\n");
        let violations = check_no_string_annotations(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn multiple_string_annotations_all_caught() {
        let parsed = parse("def transform(data: \"Input\", config: \"Config\") -> \"Output\":\n    pass\n");
        let violations = check_no_string_annotations(&parsed, &default_config());
        assert_eq!(violations.len(), 3);
    }
}
